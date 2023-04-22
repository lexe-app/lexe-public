use std::panic::UnwindSafe;

use flutter_rust_bridge::{
    handler::{
        Executor, ReportDartErrorHandler, SimpleHandler, ThreadPoolExecutor,
    },
    rust2dart::TaskCallback,
    IntoDart, SyncReturn, WrapInfo,
};

pub(crate) type LxHandler = SimpleHandler<LxExecutor, ReportDartErrorHandler>;

pub(crate) struct LxExecutor {
    worker_pool: ThreadPoolExecutor<ReportDartErrorHandler>,
}

impl LxExecutor {
    pub(crate) fn new(error_handler: ReportDartErrorHandler) -> Self {
        Self {
            worker_pool: ThreadPoolExecutor::new(error_handler),
        }
    }
}

impl Executor for LxExecutor {
    fn execute<TaskFn, TaskRet>(&self, wrap_info: WrapInfo, task: TaskFn)
    where
        TaskFn: FnOnce(TaskCallback) -> anyhow::Result<TaskRet>
            + Send
            + UnwindSafe
            + 'static,
        TaskRet: IntoDart,
    {
        self.worker_pool.execute(wrap_info, task)
    }

    fn execute_sync<SyncTaskFn, TaskRet>(
        &self,
        wrap_info: WrapInfo,
        sync_task: SyncTaskFn,
    ) -> anyhow::Result<SyncReturn<TaskRet>>
    where
        SyncTaskFn:
            FnOnce() -> anyhow::Result<SyncReturn<TaskRet>> + UnwindSafe,
        TaskRet: IntoDart,
    {
        self.worker_pool.execute_sync(wrap_info, sync_task)
    }
}
