use common::ln::channel::LxOutPoint;
use lightning::chain::chainmonitor::MonitorUpdateId;

pub struct LxChannelMonitorUpdate {
    pub funding_txo: LxOutPoint,
    pub update_id: MonitorUpdateId,
}
