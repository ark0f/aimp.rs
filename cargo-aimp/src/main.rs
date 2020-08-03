use anyhow::{Context, Result};
use cargo_metadata::{Artifact, Message, MetadataCommand};
use serde::Deserialize;
use std::process::exit;
use std::{
    env,
    ffi::OsStr,
    fmt, fs,
    fs::File,
    io,
    io::BufReader,
    mem,
    mem::MaybeUninit,
    ops::Deref,
    os::raw::c_void,
    path::PathBuf,
    process::{Child, Command, Stdio},
};
use structopt::StructOpt;
use winapi::_core::str::FromStr;
use winapi::{
    shared::minwindef::{FALSE, MAX_PATH},
    um::{
        handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
        processthreadsapi::{OpenProcess, TerminateProcess},
        tlhelp32::{
            CreateToolhelp32Snapshot, Process32First, Process32Next, PROCESSENTRY32,
            TH32CS_SNAPPROCESS,
        },
        winnt::PROCESS_TERMINATE,
    },
};
use zip::{write::FileOptions, ZipWriter};

const AIMP_DIR: &'static str = "C:/Program Files (x86)/AIMP";
const AIMP_EXE: &'static str = "AIMP.exe";
const AIMP_TOML: &'static str = "AIMP.toml";

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("cdylib crate type is required")]
    InvalidCrateType,
    #[error("Failed to create toolhelp snapshot")]
    ToolhelpSnapshot,
    #[error("Process32First failed")]
    Process32First,
    #[error("Failed to open process")]
    OpenProcess,
    #[error("Color option is invalid")]
    InvalidColorOption,
}

#[derive(Debug)]
enum Color {
    Auto,
    Always,
    Never,
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Color::Auto => "auto",
            Color::Always => "always",
            Color::Never => "never",
        }
        .fmt(f)
    }
}

impl FromStr for Color {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            "never" => Ok(Self::Never),
            _ => Err(Error::InvalidColorOption),
        }
    }
}

#[derive(Debug, StructOpt)]
/// Builds DLL, pack into zip archive and run AIMP with attached console
struct Args {
    subcommand: String,
    #[structopt(long = "package")]
    package: Option<String>,
    #[structopt(long = "no-run")]
    /// Don't kill and don't run AIMP
    no_run: bool,
    #[structopt(long = "release")]
    /// Builds DLL in release mode and pack it into zip archive
    release: bool,
    #[structopt(long = "color", default_value = "auto")]
    color: Color,
    #[structopt(long = "target-dir")]
    target_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Toml {
    #[serde(default = "default_langs")]
    langs: PathBuf,
    #[serde(default)]
    dlls: Vec<PathBuf>,
    #[serde(default = "default_exe_dir")]
    exe_dir: PathBuf,
}

impl Default for Toml {
    fn default() -> Self {
        Self {
            langs: default_langs(),
            dlls: vec![],
            exe_dir: default_exe_dir(),
        }
    }
}

fn default_langs() -> PathBuf {
    PathBuf::from("langs")
}

fn default_exe_dir() -> PathBuf {
    PathBuf::from(AIMP_DIR)
}

fn get_package_name(package_flag: Option<String>) -> Result<String> {
    let metadata = MetadataCommand::new().no_deps().exec()?;
    let package = metadata
        .packages
        .into_iter()
        .find(|package| {
            Some(&package.name) == package_flag.as_ref() || {
                let mut path = PathBuf::from(&package.manifest_path);
                path.pop();
                path == env::current_dir().unwrap()
            }
        })
        .map(|package| package.name)
        .unwrap();
    Ok(package)
}

fn cargo_build(
    package: &str,
    release: bool,
    color: Color,
    target_dir: Option<String>,
) -> Result<Child> {
    let mut cmd = Command::new("cargo");
    cmd.args(&[
        "build",
        "--message-format=json",
        "--package",
        package,
        "--color",
        &color.to_string(),
    ])
    .stdout(Stdio::piped());
    if release {
        cmd.arg("--release");
    }
    if let Some(dir) = target_dir {
        cmd.args(&["--target-dir", &dir]);
    }
    let child = cmd.spawn()?;
    Ok(child)
}

fn get_package_artifact(package: &str, mut child: Child) -> Result<Artifact> {
    let reader = BufReader::new(child.stdout.take().unwrap());
    let artifact = Message::parse_stream(reader)
        .into_iter()
        .find_map(|msg| {
            msg.map(|msg| match msg {
                Message::CompilerArtifact(artifact)
                    if artifact.package_id.repr.starts_with(package)
                        && artifact.target.src_path.ends_with("lib.rs") =>
                {
                    Some(artifact)
                }
                Message::CompilerMessage(msg) => {
                    println!("{}", msg);
                    None
                }
                _ => None,
            })
            .ok()
            .flatten()
        })
        .unwrap();
    Ok(artifact)
}

fn pack(package: &str, filenames: Vec<PathBuf>, toml: &Toml) -> Result<()> {
    let dll = filenames
        .into_iter()
        .find(|path| path.extension() == Some(OsStr::new("dll")))
        .unwrap();

    let mut zip = dll.clone();
    zip.set_extension("zip");

    let mut dll_file = File::open(dll)?;
    let archive = File::create(zip)?;
    let mut archive = ZipWriter::new(archive);

    let plugin_dir = PathBuf::from(package);

    let mut dll = plugin_dir.join(package);
    dll.set_extension("dll");
    archive.start_file_from_path(dll.as_path(), FileOptions::default())?;
    io::copy(&mut dll_file, &mut archive)?;

    for dll in &toml.dlls {
        let dll_name = dll.file_name().unwrap();

        archive
            .start_file_from_path(plugin_dir.join(dll_name).as_path(), FileOptions::default())?;
        let mut dll_file = File::open(dll).context("Additional DLL for plugin")?;
        io::copy(&mut dll_file, &mut archive)?;
    }

    if toml.langs.exists() {
        let langs_dir = plugin_dir.join("Langs");
        for lang in fs::read_dir(&toml.langs)? {
            let lang = lang?.path();
            let lang_file_name = lang.file_name().unwrap();

            archive.start_file_from_path(
                langs_dir.join(lang_file_name).as_path(),
                FileOptions::default(),
            )?;
            let mut lang = File::open(lang)?;
            io::copy(&mut lang, &mut archive)?;
        }
    }

    archive.finish()?;

    Ok(())
}

unsafe fn kill_aimp() -> Result<()> {
    struct Snapshot(*mut c_void);

    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    impl Deref for Snapshot {
        type Target = *mut c_void;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if snapshot == INVALID_HANDLE_VALUE {
        Err(Error::ToolhelpSnapshot)?;
    }
    let snapshot = Snapshot(snapshot);

    let mut entry: PROCESSENTRY32 = MaybeUninit::zeroed().assume_init();
    entry.dwSize = mem::size_of::<PROCESSENTRY32>() as u32;

    if Process32First(*snapshot, &mut entry) == FALSE {
        Err(Error::Process32First)?;
    }

    loop {
        let exe_file: [u8; MAX_PATH] = mem::transmute(entry.szExeFile);
        if exe_file.starts_with(AIMP_EXE.as_bytes()) {
            let process = OpenProcess(PROCESS_TERMINATE, FALSE, entry.th32ProcessID);
            if process == INVALID_HANDLE_VALUE {
                Err(Error::OpenProcess)?;
            }
            TerminateProcess(process, 0);
            CloseHandle(process);
        }

        if Process32Next(*snapshot, &mut entry) == FALSE {
            break;
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let args: Args = Args::from_args();

    let toml = PathBuf::from(AIMP_TOML);
    let toml = if toml.exists() {
        let aimp = fs::read_to_string("Aimp.toml")?;
        toml::from_str(&aimp)?
    } else {
        Toml::default()
    };

    let aimp_dir = &toml.exe_dir;

    let package = get_package_name(args.package)?;
    let child = cargo_build(&package, args.release, args.color, args.target_dir)?;
    let artifact = get_package_artifact(&package, child)?;

    if !artifact
        .target
        .crate_types
        .into_iter()
        .any(|kind| kind == "cdylib")
    {
        Err(Error::InvalidCrateType)?;
    }

    pack(&package, artifact.filenames, &toml)?;

    if !args.release && !args.no_run {
        unsafe {
            kill_aimp()?;
        }

        let plugin_dir = aimp_dir.join("Plugins").join(&package);
        if plugin_dir.exists() {
            fs::remove_dir_all(plugin_dir)?;
        }

        let status = Command::new(aimp_dir.join(AIMP_EXE))
            .envs(env::vars())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()?
            .status;

        if !status.success() {
            if let Some(code) = status.code() {
                exit(code);
            }
        }
    }

    Ok(())
}
