use aimp::{
    internet::{HttpClient, HttpError},
    msg_box, Plugin, PluginCategory, PluginInfo,
};
use http::HeaderValue;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("{0}")]
    Aimp(
        #[from]
        #[source]
        aimp::Error,
    ),
    #[error("{0}")]
    Http(
        #[from]
        #[source]
        HttpError,
    ),
}

struct MyPlugin;

impl Plugin for MyPlugin {
    const INFO: PluginInfo = PluginInfo {
        name: "AAA Test Rust Plugin",
        author: "ark0f",
        short_description: "Short",
        full_description: Some("Full"),
        category: PluginCategory::Addons,
    };

    type Error = Error;

    fn new() -> Result<Self> {
        let task = HttpClient::get("https://google.com")?
            .header(
                http::header::CONTENT_LANGUAGE,
                HeaderValue::from_static("ru-RU"),
            )
            .send()?;
        msg_box!("Task started");
        task.cancel_and_wait();

        Ok(MyPlugin)
    }

    fn finish(self) -> Result<()> {
        Ok(())
    }
}

aimp::main!(MyPlugin);
