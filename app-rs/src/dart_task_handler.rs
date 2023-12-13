use flutter_rust_bridge::handler::{
    ReportDartErrorHandler, SimpleHandler, ThreadPoolExecutor,
};

pub(crate) type LxHandler = SimpleHandler<
    ThreadPoolExecutor<ReportDartErrorHandler>,
    ReportDartErrorHandler,
>;
