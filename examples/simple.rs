use aimp::{
    file::{FileFormat, FileFormatWrapper, FileFormatsCategory},
    internet::{HttpClient, HttpClientPriorityFlags, HttpError},
    threading::THREADS,
    Plugin, PluginCategory, PluginInfo, CORE,
};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("{0}")]
    Http(
        #[from]
        #[source]
        HttpError,
    ),
}

struct SimpleFileFormats; // additional supported file formats

impl FileFormat for SimpleFileFormats {
    const DESCRIPTION: &'static str = "Some formats we support";
    const EXTS: &'static [&'static str] = &["*.txt", "*.rs"];
    const FLAGS: FileFormatsCategory = FileFormatsCategory::AUDIO;
}

struct SimplePlugin;

impl Plugin for SimplePlugin {
    const INFO: PluginInfo = PluginInfo {
        name: "Simple plugin",
        author: "ark0f",
        short_description: "This is a simple plugin",
        full_description: None,
        category: || PluginCategory::ADDONS,
    };
    type Error = Error;

    fn new() -> Result<Self, Self::Error> {
        println!("Hi!");

        let resp = HttpClient::get("https://google.com")?
            .priority(HttpClientPriorityFlags::High)
            .send_and_wait()?;
        println!("{:?}", resp);

        let threads = THREADS.get();
        threads.block_in_main(async {
            println!("Hello from main thread!");
        });

        let core = CORE.get();
        core.register_extension(FileFormatWrapper(SimpleFileFormats));

        Ok(Self)
    }

    fn finish(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

aimp::main!(SimplePlugin);
