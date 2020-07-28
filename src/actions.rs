pub use iaimp::{HotkeyModifier, Key};

use crate::{
    core::Extension, error::HresultExt, prop_list, prop_list::PropertyList, util::Service,
    AimpString, CORE,
};
use iaimp::{
    com_wrapper, ActionProp, ComInterface, ComInterfaceQuerier, ComPtr, ComRc, IAIMPAction,
    IAIMPActionEvent, IAIMPServiceActionManager, IAIMPString, IUnknown, IID,
};
use std::mem::MaybeUninit;
use winapi::shared::winerror::{E_INVALIDARG, S_OK};

pub(crate) static ACTION_MANAGER_SERVICE: Service<ActionManagerService> = Service::new();

pub(crate) struct ActionManagerService(ComPtr<dyn IAIMPServiceActionManager>);

impl ActionManagerService {
    pub fn get_by_id(&self, id: AimpString) -> Option<Action> {
        unsafe {
            let mut action = MaybeUninit::uninit();
            let res = self.0.get_by_id(id.0, action.as_mut_ptr());
            match *res {
                S_OK => Some(Action::from_com_rc(action.assume_init())),
                E_INVALIDARG => None,
                _ => {
                    res.into_result().unwrap();
                    unreachable!()
                }
            }
        }
    }

    pub fn make_hotkey(&self, modifiers: HotkeyModifier, key: Key) -> i32 {
        unsafe { self.0.make_hotkey(modifiers, key) }
    }
}

impl From<ComPtr<dyn IAIMPServiceActionManager>> for ActionManagerService {
    fn from(ptr: ComPtr<dyn IAIMPServiceActionManager>) -> Self {
        Self(ptr)
    }
}

pub fn make_hotkey(modifiers: HotkeyModifier, key: Key) -> i32 {
    ACTION_MANAGER_SERVICE.get().make_hotkey(modifiers, key)
}

prop_list! {
    list: Action(ComRc<dyn IAIMPAction>),
    prop: ActionProp,
    guard: ActionGuard,
    methods:
    custom(Custom) -> Option<ComRc<dyn IUnknown>>,
    id(Id) -> AimpString,
    name(Name) -> AimpString,
    group_name(GroupName) -> AimpString,
    enabled(Enabled) -> bool,
    default_local_hotkey(DefaultLocalHotkey) -> i32,
    default_global_hotkey(DefaultGlobalHotkey) -> i32,
    default_global_hotkey2(DefaultGlobalHotkey2) -> i32,
    event(Event) -> ComRc<dyn IAIMPActionEvent>,
}

impl Action {
    fn from_com_rc(rc: ComRc<dyn IAIMPAction>) -> Self {
        Self {
            prop_list: PropertyList::from(rc),
        }
    }

    pub fn by_id<T: Into<AimpString>>(id: T) -> Option<Action> {
        ACTION_MANAGER_SERVICE.get().get_by_id(id.into())
    }
}

impl Extension for Action {
    const SERVICE_IID: IID = <dyn IAIMPServiceActionManager as ComInterface>::IID;
}

impl From<Action> for ComRc<dyn IAIMPAction> {
    fn from(action: Action) -> Self {
        (action.prop_list).0
    }
}

pub struct ActionFields {
    pub id: AimpString,
    pub name: AimpString,
    pub enabled: bool,
    pub event: ActionEventObj,
}

pub struct ActionBuilder {
    fields: ActionFields,
    custom: Option<ComRc<dyn IUnknown>>,
    group_name: Option<AimpString>,
    default_local_hotkey: i32,
    default_global_hotkey: i32,
    default_global_hotkey2: i32,
}

impl ActionBuilder {
    pub fn new(fields: ActionFields) -> Self {
        Self {
            fields,
            custom: None,
            group_name: None,
            default_local_hotkey: 0,
            default_global_hotkey: 0,
            default_global_hotkey2: 0,
        }
    }

    pub fn custom<T: Into<ComRc<U>>, U: ComInterface + ?Sized>(mut self, custom: T) -> Self {
        unsafe {
            self.custom = Some(custom.into().cast());
        }
        self
    }

    pub fn group_name<T: Into<AimpString>>(mut self, group_name: T) -> Self {
        self.group_name = Some(group_name.into());
        self
    }

    pub fn default_local_hotkey<T: Into<i32>>(mut self, default_local_hotkey: T) -> Self {
        self.default_local_hotkey = default_local_hotkey.into();
        self
    }

    pub fn default_global_hotkey<T: Into<i32>>(mut self, default_global_hotkey: T) -> Self {
        self.default_global_hotkey = default_global_hotkey.into();
        self
    }

    pub fn default_global_hotkey2<T: Into<i32>>(mut self, default_global_hotkey2: T) -> Self {
        self.default_global_hotkey2 = default_global_hotkey2.into();
        self
    }

    pub fn build(self) -> Action {
        let mut action = Action::from_com_rc(CORE.get().create().unwrap());

        let mut guard = action.update();
        guard
            .id(self.fields.id)
            .name(self.fields.name)
            .enabled(self.fields.enabled)
            .default_local_hotkey(self.default_local_hotkey)
            .default_global_hotkey(self.default_global_hotkey)
            .default_global_hotkey2(self.default_global_hotkey2);

        let wrapper =
            unsafe { com_wrapper!(self.fields.event => dyn IAIMPActionEvent).into_com_rc() };
        guard.event(wrapper);

        if let Some(custom) = self.custom {
            guard.custom(Some(custom));
        }

        if let Some(group_name) = self.group_name {
            guard.group_name(group_name);
        }

        drop(guard);

        action
    }
}

pub trait ActionEvent {
    type Data: ActionEventData;

    fn on_execute(&self, data: Self::Data);
}

pub trait ActionEventData: Sized {
    fn from_com_ptr(data: Option<ComPtr<dyn IUnknown>>) -> Self;
}

impl<T: ComInterface + ?Sized> ActionEventData for Option<ComPtr<T>> {
    fn from_com_ptr(data: Option<ComPtr<dyn IUnknown>>) -> Self {
        data.map(|data| {
            if data.check_inheritance_chain_by_ref(&T::IID) {
                unsafe { data.cast() }
            } else {
                panic!("Invalid ActionEvent::Data");
            }
        })
    }
}

impl<T: ComInterface + ?Sized> ActionEventData for ComPtr<T> {
    fn from_com_ptr(data: Option<ComPtr<dyn IUnknown>>) -> Self {
        Option::<ComPtr<T>>::from_com_ptr(data).unwrap()
    }
}

impl ActionEventData for AimpString {
    fn from_com_ptr(data: Option<ComPtr<dyn IUnknown>>) -> Self {
        AimpString(ComPtr::<dyn IAIMPString>::from_com_ptr(data).into())
    }
}

pub struct ActionEventWrapper<T>(T);

impl<T> ActionEvent for ActionEventWrapper<T>
where
    T: ActionEvent,
{
    type Data = Option<ComPtr<dyn IUnknown>>;

    fn on_execute(&self, data: Self::Data) {
        self.0.on_execute(ActionEventData::from_com_ptr(data))
    }
}

pub struct ActionEventObj(Box<dyn ActionEvent<Data = Option<ComPtr<dyn IUnknown>>>>);

impl ActionEventObj {
    pub fn new<T: ActionEvent + 'static>(event: T) -> Self {
        Self(Box::new(ActionEventWrapper(event)))
    }
}

impl ComInterfaceQuerier for ActionEventObj {}

impl IAIMPActionEvent for ActionEventObj {
    unsafe fn on_execute(&self, data: Option<ComPtr<dyn IUnknown>>) {
        self.0.on_execute(data);
    }
}
