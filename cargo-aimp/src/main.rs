use anyhow::{Context, Result};
use cargo_metadata::{Artifact, Message, MetadataCommand};
use serde::Deserialize;
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
    process::{exit, Child, Command, Stdio},
    str::FromStr,
};
use structopt::StructOpt;
use winapi::{
    shared::minwindef::{DWORD, FALSE, MAX_PATH},
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

const AIMP_ROOT_DIR: &str = "C:/Program Files (x86)/AIMP";
const AIMP_EXE: &str = "AIMP.exe";
const AIMP_TOML: &str = "AIMP.toml";

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("cdylib crate type is required")]
    InvalidCrateType,
    #[error("Color option is invalid")]
    InvalidColorOption,
    #[error("Build failed")]
    BuildFailed,
    #[error("--package and --example flags are not allowed at the same time")]
    PackageAndExample,
    #[error("Failed to create toolhelp snapshot: {0}")]
    ToolhelpSnapshot(io::Error),
    #[error("Process32First failed: {0}")]
    Process32First(io::Error),
    #[error("Failed to open process: {0}")]
    OpenProcess(io::Error),
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
/// Builds, installs plugin and runs AIMP with attached console
struct Args {
    subcommand: String,
    #[structopt(long = "package")]
    package: Option<String>,
    #[structopt(long = "example")]
    example: Option<String>,
    #[structopt(long = "no-run")]
    /// Don't kill and don't run AIMP
    no_run: bool,
    #[structopt(long = "release")]
    /// Builds DLL in release mode and pack it into zip archive
    release: bool,
    #[structopt(long = "features")]
    features: Vec<String>,
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
}

impl Default for Toml {
    fn default() -> Self {
        Self {
            langs: default_langs(),
            dlls: vec![],
        }
    }
}

fn default_langs() -> PathBuf {
    PathBuf::from("langs")
}

fn get_crate_name(package_flag: Option<&str>) -> Result<String> {
    let metadata = MetadataCommand::new().no_deps().exec()?;
    let package = match package_flag {
        Some(package) => metadata
            .packages
            .into_iter()
            .flat_map(|package| package.targets)
            .find(|target| target.name == package)
            .map(|target| target.name),
        None => metadata
            .packages
            .into_iter()
            .find(|package| {
                let mut path = PathBuf::from(&package.manifest_path);
                path.pop();
                path == env::current_dir().unwrap()
            })
            .map(|package| package.name),
    };
    Ok(package.unwrap())
}

#[derive(Debug)]
enum CrateKind {
    Package(String),
    Example(String),
}

fn cargo_build(
    crate_kind: CrateKind,
    release: bool,
    features: Vec<String>,
    color: Color,
    target_dir: Option<String>,
) -> Result<Child> {
    let mut cmd = Command::new("cargo");
    cmd.args(&[
        "build",
        "--message-format=json",
        "--color",
        &color.to_string(),
    ])
    .stdout(Stdio::piped());
    match crate_kind {
        CrateKind::Package(package) => cmd.args(&["--package", &package]),
        CrateKind::Example(example) => cmd.args(&["--example", &example]),
    };
    if release {
        cmd.arg("--release");
    }
    if !features.is_empty() {
        cmd.arg("--features");
        cmd.args(features);
    }
    if let Some(dir) = target_dir {
        cmd.args(&["--target-dir", &dir]);
    }
    let child = cmd.spawn()?;
    Ok(child)
}

fn get_package_artifact(package: String, mut child: Child) -> Result<Option<Artifact>> {
    let reader = BufReader::new(child.stdout.take().unwrap());
    let artifact = Message::parse_stream(reader)
        .into_iter()
        .find_map(|msg| {
            msg.map(|msg| match msg {
                Message::CompilerArtifact(artifact) if artifact.target.name == package => {
                    Some(artifact)
                }
                Message::CompilerMessage(msg) => {
                    println!("{}", msg);
                    None
                }
                _ => None,
            })
            .transpose()
        })
        .transpose()?;
    Ok(artifact)
}

fn remove_plugin(package: &str, plugins_dir: &PathBuf) -> io::Result<()> {
    let plugin_dir = plugins_dir.join(&package);
    if plugin_dir.exists() {
        fs::remove_dir_all(&plugin_dir)?;
    }
    fs::create_dir(plugin_dir)?;
    Ok(())
}

trait FileSystem: Sized {
    fn create_file(&mut self, path: PathBuf, file: File) -> Result<()>;
}

struct ArchiveFs(ZipWriter<File>);

impl FileSystem for ArchiveFs {
    fn create_file(&mut self, path: PathBuf, mut file: File) -> Result<()> {
        self.0
            .start_file_from_path(path.as_path(), FileOptions::default())?;
        io::copy(&mut file, &mut self.0)?;
        Ok(())
    }
}

struct RealFs(PathBuf);

impl FileSystem for RealFs {
    fn create_file(&mut self, path: PathBuf, mut file: File) -> Result<()> {
        let mut out = File::create(self.0.join(path))?;
        io::copy(&mut file, &mut out)?;
        Ok(())
    }
}

fn pack(mut fs: impl FileSystem, package: &str, dll_file: PathBuf, toml: &Toml) -> Result<()> {
    let plugin_dir = PathBuf::from(package);

    let dll_file = File::open(dll_file)?;
    let mut dll_path = plugin_dir.join(package);
    dll_path.set_extension("dll");
    fs.create_file(dll_path, dll_file)?;

    for dll in &toml.dlls {
        let dll_name = dll.file_name().unwrap();
        let dll_file = File::open(dll).context("Additional DLL for plugin")?;
        fs.create_file(plugin_dir.join(dll_name), dll_file)?;
    }

    if toml.langs.exists() {
        let langs_dir = plugin_dir.join("Langs");
        for lang in fs::read_dir(&toml.langs)? {
            let lang = lang?.path();
            let lang_file_name = lang.file_name().unwrap();
            let lang = File::open(&lang)?;
            fs.create_file(langs_dir.join(lang_file_name), lang)?;
        }
    }

    Ok(())
}

unsafe fn find_aimp() -> Result<Option<DWORD>> {
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
        Err(io::Error::last_os_error()).map_err(Error::ToolhelpSnapshot)?;
    }
    let snapshot = Snapshot(snapshot);

    let mut entry: PROCESSENTRY32 = MaybeUninit::zeroed().assume_init();
    entry.dwSize = mem::size_of::<PROCESSENTRY32>() as u32;

    if Process32First(*snapshot, &mut entry) == FALSE {
        Err(io::Error::last_os_error()).map_err(Error::Process32First)?;
    }

    let process = loop {
        let exe_file: [u8; MAX_PATH] = mem::transmute(entry.szExeFile);
        if exe_file.starts_with(AIMP_EXE.as_bytes()) {
            break Some(entry.th32ProcessID);
        }

        if Process32Next(*snapshot, &mut entry) == FALSE {
            break None;
        }
    };

    Ok(process)
}

unsafe fn kill_process(process: DWORD) -> Result<()> {
    let process = OpenProcess(PROCESS_TERMINATE, FALSE, process);
    if process == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error()).map_err(Error::OpenProcess)?;
    }
    TerminateProcess(process, 0);
    CloseHandle(process);
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

    let aimp_root_dir = env::var("CARGO_AIMP_PLAYER_ROOT_DIR")
        .map_or_else(|_| PathBuf::from(AIMP_ROOT_DIR), PathBuf::from);

    let krate = args.package.as_deref().or(args.example.as_deref());
    let package = get_crate_name(krate)?;
    let crate_kind = match (args.package.is_some(), args.example.is_some()) {
        (true, false) => CrateKind::Package(package.clone()),
        (false, true) => CrateKind::Example(package.clone()),
        (false, false) => CrateKind::Package(package.clone()),
        (true, true) => Err(Error::PackageAndExample)?,
    };
    let child = cargo_build(
        crate_kind,
        args.release,
        args.features,
        args.color,
        args.target_dir,
    )?;
    let artifact = get_package_artifact(package.clone(), child)?.ok_or(Error::BuildFailed)?;

    if !artifact
        .target
        .crate_types
        .into_iter()
        .any(|kind| kind == "cdylib")
    {
        Err(Error::InvalidCrateType)?;
    }

    let dll = artifact
        .filenames
        .into_iter()
        .find(|path| path.extension() == Some(OsStr::new("dll")))
        .unwrap();

    if args.release {
        let mut zip = dll.clone();
        zip.set_extension("zip");
        let file = File::create(zip)?;

        let fs = ArchiveFs(ZipWriter::new(file));
        pack(fs, &package, dll, &toml)?;
    } else if !args.no_run {
        unsafe {
            find_aimp()?
                .map(|process| kill_process(process))
                .transpose()?;
        }

        let plugins_dir = aimp_root_dir.join("Plugins");

        remove_plugin(&package, &plugins_dir)?;

        let fs = RealFs(plugins_dir.clone());
        pack(fs, &package, dll, &toml)?;

        let status = Command::new(aimp_root_dir.join(AIMP_EXE))
            .envs(env::vars())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()?
            .status;

        remove_plugin(&package, &plugins_dir)?;

        if !status.success() {
            if let Some(code) = status.code() {
                exit(code);
            }
        }
    }

    Ok(())
}
