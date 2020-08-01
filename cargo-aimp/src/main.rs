use anyhow::Result;
use cargo_metadata::{Artifact, Message, MetadataCommand};
use std::{
    env,
    ffi::OsStr,
    fmt, fs,
    fs::File,
    io,
    io::{BufRead, BufReader},
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
                    if artifact.package_id.repr.starts_with(package) =>
                {
                    Some(artifact)
                }
                Message::CompilerMessage(msg) => {
                    println!("{}", msg.to_string());
                    None
                }
                _ => None,
            })
            .map_or(None, |x| x)
        })
        .unwrap();
    Ok(artifact)
}

fn pack(package: &str, filenames: Vec<PathBuf>) -> Result<()> {
    let dll = filenames
        .into_iter()
        .find(|path| path.extension() == Some(OsStr::new("dll")))
        .unwrap();

    let mut zip = dll.clone();
    zip.set_extension("zip");

    let mut dll = File::open(dll)?;
    let archive = File::create(zip)?;
    let mut archive = ZipWriter::new(archive);

    let mut dll_in_zip = PathBuf::from(package).join(package);
    dll_in_zip.set_extension("dll");
    archive.start_file_from_path(dll_in_zip.as_path(), FileOptions::default())?;
    io::copy(&mut dll, &mut archive)?;

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

    let aimp_dir = env::var("AIMP_DIR")
        .ok()
        .map_or_else(|| PathBuf::from(AIMP_DIR), PathBuf::from);

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

    pack(&package, artifact.filenames)?;

    if !args.release || !args.no_run {
        unsafe {
            kill_aimp()?;
        }

        let plugin_dir = aimp_dir.join("Plugins").join(&package);
        if plugin_dir.exists() {
            fs::remove_dir_all(plugin_dir)?;
        }

        let aimp = Command::new(aimp_dir.join(AIMP_EXE))
            .envs(env::vars())
            .stdout(Stdio::piped())
            .spawn()?;
        let mut reader = BufReader::new(aimp.stdout.unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                break;
            }

            println!("{}", line);
        }
    }

    Ok(())
}
