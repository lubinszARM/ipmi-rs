use std::num::NonZeroU32;

use super::RakpErrorStatusCode;

#[derive(Debug)]
pub enum RakpMessageTwoParseError {
    NotEnoughData,
    ErrorStatusCode(ErrorStatusCode),
    UnknownErrorStatusCode(u8),
    InvalidRemoteConsoleSessionId,
}

#[derive(Debug)]
pub struct RakpMessageTwo<'a> {
    pub message_tag: u8,
    pub remote_console_session_id: NonZeroU32,
    pub managed_system_random_number: [u8; 16],
    pub managed_system_guid: [u8; 16],
    pub key_exchange_auth_code: &'a [u8],
}

impl<'a> RakpMessageTwo<'a> {
    pub fn from_data(data: &'a [u8]) -> Result<Self, RakpMessageTwoParseError> {
        if data.len() < 2 {
            return Err(RakpMessageTwoParseError::NotEnoughData);
        }

        let message_tag = data[0];
        let status_code = data[2];

        if status_code != 0 {
            return Err(ErrorStatusCode::try_from(status_code)
                .map(RakpMessageTwoParseError::ErrorStatusCode)
                .unwrap_or(RakpMessageTwoParseError::UnknownErrorStatusCode(
                    status_code,
                )));
        }

        if data.len() < 40 {
            return Err(RakpMessageTwoParseError::NotEnoughData);
        }

        let remote_console_session_id =
            if let Some(v) = NonZeroU32::new(u32::from_le_bytes(data[4..8].try_into().unwrap())) {
                v
            } else {
                return Err(RakpMessageTwoParseError::InvalidRemoteConsoleSessionId);
            };

        let managed_system_random_number = data[8..24].try_into().unwrap();
        let managed_system_guid = data[24..40].try_into().unwrap();
        let key_exchange_auth_code = &data[40..];

        Ok(Self {
            message_tag,
            remote_console_session_id,
            managed_system_random_number,
            managed_system_guid,
            key_exchange_auth_code,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u32)]
pub enum ErrorStatusCode {
    CommonRakp(RakpErrorStatusCode),
    InactiveSessionId = 0x08,
    InvalidRole = 0x09,
    UnauthorizedRoleOrPrivilegeLevelRequested = 0x0A,
    InsufficientResourcesToCreateSessionAtRequestedRole = 0x0B,
    InvalidNameLength = 0x0C,
    UnauthorizedName = 0x0D,
}

impl TryFrom<u8> for ErrorStatusCode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if let Ok(common) = TryFrom::try_from(value) {
            return Ok(ErrorStatusCode::CommonRakp(common));
        }

        let value = match value {
            0x03 => ErrorStatusCode::InactiveSessionId,
            0x09 => ErrorStatusCode::InvalidRole,
            0x0A => ErrorStatusCode::UnauthorizedRoleOrPrivilegeLevelRequested,
            0x0B => ErrorStatusCode::InsufficientResourcesToCreateSessionAtRequestedRole,
            0x0C => ErrorStatusCode::InvalidNameLength,
            0x0D => ErrorStatusCode::UnauthorizedName,
            _ => return Err(()),
        };

        Ok(value)
    }
}
