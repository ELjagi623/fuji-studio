//! Transport abstraction: anything that can run one PTP transaction.

use anyhow::Result;

pub enum DataPhase<'a> {
    /// Command + response only.
    None,
    /// Device -> host data phase.
    In,
    /// Host -> device data phase.
    Out(&'a [u8]),
}

#[derive(Debug, Clone)]
pub struct Response {
    pub code: u16,
    pub params: Vec<u32>,
    pub data: Vec<u8>,
}

impl Response {
    pub fn is_ok(&self) -> bool {
        self.code == crate::codes::resp::OK
    }
}

/// Progress callback: (bytes done, bytes total).
pub type Progress<'a> = Option<&'a mut dyn FnMut(u64, u64)>;

pub trait Transport {
    fn describe(&self) -> String;
    /// True when the OS driver owns the PTP session (e.g. Windows WPD),
    /// so the client must NOT send OpenSession/CloseSession itself.
    fn driver_manages_session(&self) -> bool;
    fn execute(
        &mut self,
        op: u16,
        params: &[u32],
        data: DataPhase<'_>,
        progress: Progress<'_>,
    ) -> Result<Response>;
}
