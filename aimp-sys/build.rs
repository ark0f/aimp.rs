use bindgen::EnumVariation;
use clang::{token::Token, Clang, Entity, Index, TranslationUnit};
use std::fmt::Display;
use std::marker::PhantomData;
use std::{
    env, fmt,
    fs::{File, OpenOptions},
    io::Write,
    iter,
    path::PathBuf,
};

const AIMP_SDK: &str = "aimp_sdk/Sources/Cpp";

trait FromEntity: Sized {
    fn from_entity(entity: Entity) -> Option<Self>;
}

trait CppItem: Sized {
    fn hpp(&self) -> String;

    fn cpp(&self) -> String;
}

trait Preprocessor: Sized + Display {
    fn dup_in_header(&self) -> bool;
}

struct GeneratorStateInclude;

struct GeneratorStatePush;

struct Generator<T> {
    name: String,
    hpp: File,
    cpp: File,
    cpp_path: PathBuf,
    _state: PhantomData<T>,
}

impl Generator<GeneratorStateInclude> {
    fn new(out_dir: &PathBuf, name: &str) -> Self {
        let cpp_path = out_dir.join(format!("{}.cpp", name));
        let cpp = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&cpp_path)
            .unwrap();
        let hpp = out_dir.join(format!("{}.hpp", name));
        let hpp = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&hpp)
            .unwrap();
        Self {
            name: name.to_string(),
            hpp,
            cpp,
            cpp_path,
            _state: PhantomData,
        }
    }

    fn preprocessor<T: Preprocessor>(mut self, item: T) -> Self {
        let s = item.to_string();
        write!(&mut self.cpp, "{}", s).unwrap();
        if item.dup_in_header() {
            write!(&mut self.hpp, "{}", s).unwrap();
        }
        self
    }

    fn push_from_tu<T: FromEntity + CppItem>(
        self,
        tu: &TranslationUnit,
    ) -> Generator<GeneratorStatePush> {
        Generator::<GeneratorStatePush> {
            name: self.name,
            hpp: self.hpp,
            cpp: self.cpp,
            cpp_path: self.cpp_path,
            _state: PhantomData,
        }
        .push_from_tu::<T>(tu)
    }
}

impl Generator<GeneratorStatePush> {
    fn push<T: CppItem>(mut self, item: T) -> Self {
        write!(&mut self.hpp, "{}", item.hpp()).unwrap();
        write!(&mut self.cpp, "{}", item.cpp()).unwrap();
        self
    }

    fn push_from_tu<T: FromEntity + CppItem>(self, tu: &TranslationUnit) -> Self {
        tu.get_entity()
            .get_children()
            .into_iter()
            .filter_map(T::from_entity)
            .fold(self, |this, t| this.push(t))
    }

    fn build(self) -> GeneratorBuild {
        let mut build = cc::Build::new();
        build.cpp(true).file(self.cpp_path);
        GeneratorBuild {
            build,
            name: self.name,
        }
    }
}

struct GeneratorBuild {
    name: String,
    build: cc::Build,
}

impl GeneratorBuild {
    fn include(mut self, path: &str) -> Self {
        self.build.include(path);
        self
    }

    fn compile(self) {
        self.build.compile(&format!("aimp_cpp_sdk_{}", self.name))
    }
}

struct Include {
    file_name: String,
    in_hpp: bool,
}

impl Include {
    fn new(file_name: &str) -> Self {
        Include {
            file_name: file_name.to_string(),
            in_hpp: false,
        }
    }

    fn in_hpp(mut self) -> Self {
        self.in_hpp = true;
        self
    }
}

impl Preprocessor for Include {
    fn dup_in_header(&self) -> bool {
        self.in_hpp
    }
}

impl fmt::Display for Include {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "#include <{}>", self.file_name)
    }
}

struct ExternConstGuid {
    name: String,
    integers: String,
}

impl FromEntity for ExternConstGuid {
    fn from_entity(entity: Entity) -> Option<Self> {
        let name = entity
            .get_name()
            .filter(|name| name.starts_with("IID_IAIMP"))?;
        let integers = entity.get_range().unwrap().tokenize()[5..]
            .iter()
            .map(Token::get_spelling)
            .collect::<Vec<String>>()
            .join(" ");
        Some(Self { name, integers })
    }
}

impl CppItem for ExternConstGuid {
    fn hpp(&self) -> String {
        format!("extern const GUID {};", self.name)
    }

    fn cpp(&self) -> String {
        format!("extern const GUID {} = {};", self.name, self.integers)
    }
}

struct ClassMethods {
    name: String,
    methods: Vec<Method>,
    hack: Option<String>,
}

impl FromEntity for ClassMethods {
    fn from_entity(entity: Entity) -> Option<Self> {
        let name = entity.get_name().filter(|name| name.starts_with("IAIMP"))?;
        let methods: Vec<Method> = entity
            .get_children()
            .into_iter()
            .filter_map(Method::new)
            .collect();

        let mut tokens: Vec<String> = entity
            .get_range()
            .unwrap()
            .tokenize()
            .into_iter()
            .map(|token| token.get_spelling())
            .collect();
        let are_methods_public = tokens
            .windows(2)
            .any(|tokens| tokens[0] == "{" && tokens[1] == "public");
        let hack: Option<String> = if !are_methods_public && !methods.is_empty() {
            let pos = tokens.iter().position(|token| token == "{").unwrap();
            tokens.remove(1);
            tokens.insert(1, "__Hack".to_string());
            tokens.insert(pos + 1, "public:".to_string());
            Some(tokens.join("\n"))
        } else {
            None
        };

        Some(Self {
            name,
            methods,
            hack,
        })
    }
}

impl CppItem for ClassMethods {
    fn hpp(&self) -> String {
        let mut out = String::new();

        for method in &self.methods {
            let args = method.hpp_args(&self.name);
            out += &format!(
                "{} WINAPI {}_{}({});",
                method.ty, self.name, method.name, args
            );
        }

        out
    }

    fn cpp(&self) -> String {
        let mut out = String::new();

        for method in &self.methods {
            let hpp_args = method.hpp_args(&self.name);
            let cpp_args = method.cpp_args();

            let func = if let Some(hack) = &self.hack {
                format!("{} WINAPI {}_{method}({}) {{ {}; __Hack* ThisHack = reinterpret_cast<__Hack *>(This); return ThisHack->{method}({}); }}\n",
                        method.ty,
                        self.name,
                        hpp_args,
                        hack,
                        cpp_args,
                        method = method.name
                )
            } else {
                format!(
                    "{} WINAPI {}_{method}({}) {{ return This->{method}({}); }}\n",
                    method.ty,
                    self.name,
                    hpp_args,
                    cpp_args,
                    method = method.name
                )
            };

            out += &func;
        }

        out
    }
}

struct Method {
    ty: String,
    name: String,
    args: Vec<Arg>,
}

impl Method {
    fn new(entity: Entity) -> Option<Self> {
        if !entity.is_pure_virtual_method() {
            return None;
        }

        let ty = entity.get_result_type().unwrap().get_display_name();
        let name = entity.get_name().unwrap();
        let args = entity
            .get_arguments()
            .unwrap()
            .into_iter()
            .map(|arg| Arg {
                ty: arg.get_type().unwrap().get_display_name().replace(
                    "BOOL (IAIMPPlaylistItem *, void *) __attribute__((stdcall))",
                    "TAIMPPlaylistDeleteProc *",
                ), // I don't know why clang doesn't return name of exactly this callback
                name: arg.get_name().unwrap_or_else(|| "BlankName".to_string()),
            })
            .collect();

        Some(Self { ty, name, args })
    }

    fn hpp_args(&self, class_name: &str) -> String {
        iter::once(&Arg {
            ty: format!("{} *", class_name),
            name: "This".to_string(),
        })
        .chain(&self.args)
        .map(|arg| format!("{} {}", arg.ty, arg.name))
        .collect::<Vec<String>>()
        .join(", ")
    }

    fn cpp_args(&self) -> String {
        self.args
            .iter()
            .map(|arg| arg.name.clone())
            .collect::<Vec<String>>()
            .join(", ")
    }
}

struct Arg {
    ty: String,
    name: String,
}

fn main() {
    println!("cargo:rerun-if-changed=aimp_sdk.hpp");
    println!("cargo:rerun-if-changed=wrapper.hpp");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let clang = Clang::new().unwrap();
    let index = Index::new(&clang, false, false);
    let tu = index
        .parser("aimp_sdk.hpp")
        .arguments(&["-xc++", &format!("-I{}", AIMP_SDK)])
        .parse()
        .unwrap();

    Generator::new(&out_dir, "iids")
        .preprocessor(Include::new("guiddef.h").in_hpp())
        .push_from_tu::<ExternConstGuid>(&tu)
        .build()
        .compile();

    Generator::new(&out_dir, "util")
        .preprocessor(Include::new("aimp_sdk.hpp"))
        .push_from_tu::<ClassMethods>(&tu)
        .build()
        .include(".")
        .include(AIMP_SDK)
        .compile();

    bindgen::Builder::default()
        .header("wrapper.hpp")
        .clang_arg("-xc++")
        .clang_arg("-I.")
        .clang_arg(format!("-I{}", AIMP_SDK))
        .clang_arg(format!("-I{}", out_dir.display()))
        // custom
        .whitelist_function("IAIMP.*")
        // SDK
        .whitelist_type("IAIMP.*")
        //.whitelist_var("IID_IAIMP.*")
        .whitelist_var("AIMP.*")
        .default_enum_style(EnumVariation::Rust {
            non_exhaustive: false,
        })
        .derive_partialeq(true)
        .generate_inline_functions(true)
        .generate()
        .unwrap()
        .write_to_file(out_dir.join("bindings.rs"))
        .unwrap();

    bindgen::Builder::default()
        .header(out_dir.join("iids.hpp").display().to_string())
        .whitelist_var("IID_IAIMP.*")
        .blacklist_type("GUID")
        .blacklist_type("_GUID")
        .generate()
        .unwrap()
        .write_to_file(out_dir.join("bindings_iids.rs"))
        .unwrap();
}
