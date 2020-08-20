use crate::{Error, Plugin, PluginCategory, PluginInfo};
use std::{cell::RefCell, process::exit};
use tester::TestDescAndFn;

#[doc(hidden)]
thread_local! {
    pub static TEST_FNS: RefCell<Option<Vec<TestDescAndFn>>> = RefCell::new(None);
}

pub struct TesterPlugin;

impl Plugin for TesterPlugin {
    const INFO: PluginInfo = PluginInfo {
        name: "Tester plugin",
        author: "ark0f",
        short_description: "Tester plugin for AIMP SDK in Rust",
        full_description: None,
        category: || PluginCategory::ADDONS,
    };
    type Error = Error;

    fn new() -> Result<Self, Self::Error> {
        TEST_FNS.with(|fns| {
            let fns = fns.borrow_mut().take().unwrap_or_default();
            tester::test_main(&[], fns, None);
        });
        exit(0)
    }

    fn finish(self) -> Result<(), Self::Error> {
        Ok(())
    }
}
