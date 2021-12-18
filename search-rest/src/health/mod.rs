mod handler;
mod routes;

use serde::{Serialize, Serializer};

pub use routes::routes;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Services {
    index: ServiceStatus,
    api: ServiceStatus,
}

#[derive(Debug, Clone)]
pub enum ServiceStatus {
    Ok,
    Warning,
    Failure,
}

impl ServiceStatus {
    fn value(&self) -> u8 {
        match self {
            ServiceStatus::Ok => 0,
            ServiceStatus::Warning => 1,
            ServiceStatus::Failure => 2,
        }
    }
}

impl Serialize for ServiceStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.value())
    }
}
