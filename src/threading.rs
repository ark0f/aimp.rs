use crate::{error::HresultExt, util::Static};
use iaimp::IAIMPTaskPriorityVTable;
use iaimp::{
    com_wrapper, ComPtr, ComVTables, IAIMPServiceThreads, IAIMPTask, IAIMPTaskOwner,
    IAIMPTaskPriority, IAIMPTaskVTable, IUnknown, ServiceThreadsFlags, TaskPriority,
};
use std::future::Future;
use std::mem::MaybeUninit;
use std::num::NonZeroUsize;
use std::rc::Rc;
use winapi::_core::cell::RefCell;
use winapi::_core::pin::Pin;
use winapi::_core::task::{Context, Poll};
use winapi::shared::winerror::E_FAIL;
use winapi::shared::{
    basetsd::DWORD_PTR,
    winerror::{HRESULT, S_OK},
};

static mut SERVICE_THREADS: Static<ServiceThreads> = Static::new();

pub(crate) fn init(threads: ComPtr<dyn IAIMPServiceThreads>) {
    unsafe { SERVICE_THREADS.init(ServiceThreads { inner: threads }) }
}

pub fn get() -> &'static ServiceThreads {
    unsafe { SERVICE_THREADS.get() }
}

pub struct ServiceThreads {
    inner: ComPtr<dyn IAIMPServiceThreads>,
}

impl ServiceThreads {
    fn execute_in_main_thread<T>(&self, task: Task<T>, flags: ServiceThreadsFlags)
    where
        T: Future<Output = ()> + Send + 'static,
    {
        let wrapper = TaskWrapper::new_raw(task);
        unsafe {
            self.inner
                .execute_in_main_thread(wrapper, flags)
                .into_result()
                .unwrap();
        }
    }

    pub fn block_in_main<T>(&self, task: T)
    where
        T: Into<Task<T>> + Future<Output = ()> + Send + 'static,
    {
        self.execute_in_main_thread(task.into(), ServiceThreadsFlags::WAIT_FOR)
    }

    pub fn spawn_in_main<T>(&self, task: T)
    where
        T: Into<Task<T>> + Future<Output = ()> + Send + 'static,
    {
        self.execute_in_main_thread(task.into(), ServiceThreadsFlags::NONE)
    }

    pub fn spawn<T>(&self, task: T) -> TaskHandle
    where
        T: Into<Task<T>> + Future<Output = ()> + Send + 'static,
    {
        unsafe {
            let mut handle = MaybeUninit::uninit();
            let wrapper = TaskWrapper::new_raw(task.into());
            self.inner
                .execute_in_thread(wrapper, handle.as_mut_ptr())
                .into_result()
                .unwrap();
            TaskHandle(NonZeroUsize::new(handle.assume_init()))
        }
    }
}

pub struct Task<T> {
    fut: T,
    priority: TaskPriority,
}

impl<T> Task<T> {
    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }
}

impl<T> From<T> for Task<T> {
    fn from(fut: T) -> Self {
        Self {
            fut,
            priority: Default::default(),
        }
    }
}

impl<T> Future for Task<T>
where
    T: Future<Output = ()> + Send + 'static,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe { self.map_unchecked_mut(|s| &mut s.fut).poll(cx) }
    }
}

pub struct TaskWrapper<T> {
    inner: Rc<RefCell<Option<Task<T>>>>,
}

impl<T> TaskWrapper<T>
where
    T: Future<Output = ()> + Send + 'static,
{
    fn new(task: Task<T>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(Some(task))),
        }
    }

    fn new_raw(task: Task<T>) -> ComPtr<dyn IAIMPTask> {
        let wrapper = TaskWrapper::new(task);
        let wrapper =
            com_wrapper!(wrapper => TaskWrapper<Task<T>>: IAIMPTaskVTable, IAIMPTaskPriorityVTable);
        unsafe { wrapper.into_com_ptr() }
    }
}

impl<T> ComVTables for TaskWrapper<T> {
    type Pointers = (*mut IAIMPTaskVTable, *mut IAIMPTaskPriorityVTable);
}

impl<T> IAIMPTask for TaskWrapper<T>
where
    T: Future<Output = ()> + Send + 'static,
{
    unsafe fn execute(&self, owner: ComPtr<dyn IAIMPTaskOwner>) -> HRESULT {
        let mut fut = Box::pin(self.inner.borrow_mut().take().unwrap());

        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);

        loop {
            if owner.is_canceled() != 0 {
                break E_FAIL;
            }

            if let Poll::Ready(()) = fut.as_mut().poll(&mut cx) {
                break S_OK;
            }
        }
    }
}

impl<T> IAIMPTaskPriority for TaskWrapper<T> {
    unsafe fn get_priority(&self) -> TaskPriority {
        self.inner.borrow().as_ref().unwrap().priority
    }
}

/// A handle you get from [`ServiceThreads::spawn`](ServiceThreads::spawn)
///
/// The handle will wait task if you will not cancel and/or wait it
///
/// [`ServiceThreads::spawn`]: ServiceThreads::spawn
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct TaskHandle(Option<NonZeroUsize>);

impl TaskHandle {
    pub fn cancel(mut self) {
        unsafe {
            SERVICE_THREADS
                .get()
                .inner
                .cancel(self.take(), ServiceThreadsFlags::NONE)
                .into_result()
                .unwrap();
        }
    }

    pub fn cancel_and_wait(mut self) {
        unsafe {
            SERVICE_THREADS
                .get()
                .inner
                .cancel(self.take(), ServiceThreadsFlags::WAIT_FOR)
                .into_result()
                .unwrap();
        }
    }

    fn wait_by_ref(&mut self) {
        unsafe {
            SERVICE_THREADS
                .get()
                .inner
                .wait_for(self.take())
                .into_result()
                .unwrap();
        }
    }

    pub fn wait(mut self) {
        self.wait_by_ref();
    }

    /// # Safety
    ///
    /// Handle can be invalid if callee canceled or waited the task
    pub unsafe fn as_raw(&self) -> DWORD_PTR {
        self.0.unwrap().get()
    }

    fn take(&mut self) -> DWORD_PTR {
        self.0.take().unwrap().get()
    }
}

impl Drop for TaskHandle {
    fn drop(&mut self) {
        if self.0.as_ref().is_some() {
            self.wait_by_ref();
        }
    }
}
