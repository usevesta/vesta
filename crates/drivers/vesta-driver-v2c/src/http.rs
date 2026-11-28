//! Plain HTTP to the charger. Every request is a `GET` with a timeout, a
//! fresh connection (the charger's small web server and the Wi-Fi extender
//! it hangs off do not like idle keep-alive connections), no proxy and no
//! redirects.

use std::time::Duration;

use vesta_core::DriverError;

use crate::api::{MAX_RESPONSE_BYTES, REALTIME_PATH, Reading, Write};
use crate::config::Address;

#[derive(Debug, Clone)]
pub(crate) struct Http {
    client: reqwest::Client,
}

impl Http {
    pub(crate) fn new(timeout: Duration) -> Result<Self, DriverError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout.min(Duration::from_secs(2)))
            .pool_max_idle_per_host(0)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("vesta")
            .build()
            .map_err(|e| {
                DriverError::InvalidConfig(format!("cannot create the HTTP client: {e}"))
            })?;
        Ok(Self { client })
    }

    /// `GET` `path` and return the body (at most [`MAX_RESPONSE_BYTES`]).
    async fn get(&self, address: &Address, path: &str) -> Result<Vec<u8>, DriverError> {
        let mut response = self
            .client
            .get(address.url(path))
            .send()
            .await
            .map_err(request_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(DriverError::Protocol(format!(
                "{path} answered HTTP {}",
                status.as_u16()
            )));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(DriverError::Protocol(format!("{path} answered too much")));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    pub(crate) async fn read(&self, address: &Address) -> Result<Reading, DriverError> {
        Reading::parse(&self.get(address, REALTIME_PATH).await?)
    }

    /// Sends one write. The charger answers `OK` when it took it.
    pub(crate) async fn write(&self, address: &Address, write: Write) -> Result<(), DriverError> {
        let path = write.path();
        let body = self.get(address, &path).await?;
        if body.trim_ascii() == b"OK" {
            Ok(())
        } else {
            Err(DriverError::Protocol(format!(
                "{path} was not accepted (the charger did not answer OK)"
            )))
        }
    }
}

fn request_error(error: reqwest::Error) -> DriverError {
    if error.is_timeout() {
        DriverError::Timeout
    } else if error.is_connect() {
        DriverError::Offline
    } else {
        DriverError::Protocol(format!("request failed: {error}"))
    }
}
