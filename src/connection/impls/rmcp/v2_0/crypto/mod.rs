mod authentication;
pub use authentication::AuthenticationAlgorithm;

mod confidentiality;
pub use confidentiality::ConfidentialityAlgorithm;

mod integrity;
pub use integrity::IntegrityAlgorithm;

mod sha1;

use self::sha1::RunningHmac;

use super::{
    messages::OpenSessionResponse, Message, PayloadType, RakpMessage1, RakpMessage2, ReadError,
    WriteError,
};

#[derive(Debug, Clone, PartialEq)]
pub enum CryptoUnwrapError {
    NotEnoughData,
    MismatchingEncryptionState,
    MismatchingAuthenticationState,
    IncorrectPayloadLen,
}

#[allow(unused)]
struct Keys {
    sik: [u8; 20],
    k1: [u8; 20],
    k2: [u8; 20],
    k3: [u8; 20],
}

impl core::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keys").finish()
    }
}

impl Keys {
    pub fn from_sik(sik: [u8; 20]) -> Self {
        Self {
            sik,
            k1: RunningHmac::new(&sik).feed(&[0x01; 20]).finalize(),
            k2: RunningHmac::new(&sik).feed(&[0x02; 20]).finalize(),
            k3: RunningHmac::new(&sik).feed(&[0x03; 20]).finalize(),
        }
    }
}

pub struct CryptoState {
    confidentiality_algorithm: ConfidentialityAlgorithm,
    authentication_algorithm: AuthenticationAlgorithm,
    integrity_algorithm: IntegrityAlgorithm,
    kg: Option<[u8; 20]>,
    password: Vec<u8>,
    keys: Option<Keys>,
}

impl core::fmt::Debug for CryptoState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptoState")
            .field("confidentiality_algorithm", &self.confidentiality_algorithm)
            .field("authentication_algorithm", &self.authentication_algorithm)
            .field("integrity_algorithm", &self.integrity_algorithm)
            .field("kg", &"<redacted>")
            .field("password", &"<redacted>")
            .field("keys", &self.keys)
            .finish()
    }
}

impl Default for CryptoState {
    fn default() -> Self {
        Self {
            kg: None,
            confidentiality_algorithm: ConfidentialityAlgorithm::None,
            authentication_algorithm: AuthenticationAlgorithm::RakpNone,
            integrity_algorithm: IntegrityAlgorithm::None,
            password: Vec::new(),
            keys: None,
        }
    }
}

impl CryptoState {
    pub fn k1(&self) -> &[u8; 20] {
        &self.keys.as_ref().unwrap().k1
    }

    pub fn sik(&self) -> &[u8; 20] {
        &self.keys.as_ref().unwrap().sik
    }

    pub fn new(kg: Option<[u8; 20]>, password: &[u8], response: &OpenSessionResponse) -> Self {
        Self {
            kg,
            confidentiality_algorithm: response.confidentiality_payload,
            authentication_algorithm: response.authentication_payload,
            integrity_algorithm: response.integrity_payload,
            password: password.to_vec(),
            keys: None,
        }
    }

    pub fn encrypted(&self) -> bool {
        self.confidentiality_algorithm != ConfidentialityAlgorithm::None
    }

    pub fn authenticated(&self) -> bool {
        self.integrity_algorithm != IntegrityAlgorithm::None
    }

    pub fn validate(&mut self, m1: &RakpMessage1, m2: &RakpMessage2) -> Option<Vec<u8>> {
        match self.authentication_algorithm {
            AuthenticationAlgorithm::RakpNone => todo!(),
            AuthenticationAlgorithm::RakpHmacSha1 => {
                self.validate_hmac_sha1(m1, m2).map(|v| v.to_vec())
            }
            AuthenticationAlgorithm::RakpHmacMd5 => todo!(),
            AuthenticationAlgorithm::RakpHmacSha256 => todo!(),
        }
    }

    fn kg(&self) -> &[u8] {
        self.kg
            .as_ref()
            .map(|v| &v[..])
            .unwrap_or(self.password.as_ref())
    }

    fn validate_hmac_sha1(&mut self, m1: &RakpMessage1, m2: &RakpMessage2) -> Option<[u8; 20]> {
        let privilege_level_byte = u8::from(m1.requested_maximum_privilege_level);

        let hmac_output = RunningHmac::new(&self.password)
            .feed(&m2.remote_console_session_id.get().to_le_bytes())
            .feed(&m1.managed_system_session_id.get().to_le_bytes())
            .feed(&m1.remote_console_random_number)
            .feed(&m2.managed_system_random_number)
            .feed(&m2.managed_system_guid)
            .feed(&[privilege_level_byte, m1.username.len()])
            .feed(&m1.username)
            .finalize();

        if &hmac_output == m2.key_exchange_auth_code {
            let sik = RunningHmac::new(self.kg())
                .feed(&m1.remote_console_random_number)
                .feed(&m2.managed_system_random_number)
                .feed(&[privilege_level_byte, m1.username.len()])
                .feed(&m1.username)
                .finalize();

            self.keys = Some(Keys::from_sik(sik));

            let output = RunningHmac::new(&self.password)
                .feed(&m2.managed_system_random_number)
                .feed(&m2.remote_console_session_id.get().to_le_bytes())
                .feed(&[privilege_level_byte, m1.username.len()])
                .feed(&m1.username)
                .finalize();

            Some(output)
        } else {
            None
        }
    }

    pub fn read_payload(&mut self, data: &[u8]) -> Result<Message, ReadError> {
        if data.len() < 10 {
            return Err(ReadError::NotEnoughData);
        }

        if data[0] != 0x06 {
            return Err(ReadError::NotIpmiV2_0);
        }

        let encrypted = (data[1] & 0x80) == 0x80;
        let authenticated = (data[1] & 0x40) == 0x40;
        let ty = PayloadType::try_from(data[1] & 0x3F)
            .map_err(|_| ReadError::InvalidPayloadType(data[1] & 0x3F))?;

        let session_id = u32::from_le_bytes(data[2..6].try_into().unwrap());
        let session_sequence_number = u32::from_le_bytes(data[6..10].try_into().unwrap());

        let data = &data[10..];

        if data.len() < 2 {
            return Err(CryptoUnwrapError::NotEnoughData.into());
        }

        if self.encrypted() != encrypted {
            return Err(CryptoUnwrapError::MismatchingEncryptionState.into());
        }

        if self.authenticated() != authenticated {
            return Err(CryptoUnwrapError::MismatchingAuthenticationState.into());
        }

        let data_len = u16::from_le_bytes(data[..2].try_into().unwrap());
        let data = &data[2..];

        if data_len as usize == data.len() {
            // Strip off PAD byte when the message is not out-of-session
            let data = if session_id != 0 && session_sequence_number != 0 {
                &data[..data.len() - 1]
            } else {
                data
            };

            Ok(Message {
                ty,
                session_id,
                session_sequence_number,
                payload: data.to_vec(),
            })
        } else {
            Err(CryptoUnwrapError::IncorrectPayloadLen.into())
        }
    }

    pub fn write_message(
        &mut self,
        message: &Message,
        buffer: &mut Vec<u8>,
    ) -> Result<(), WriteError> {
        assert_eq!(buffer.len(), 4, "Buffer must only contain RMCP header.");

        buffer.push(0x06);

        let encrypted = (self.encrypted() as u8) << 7;
        let authenticated = (self.authenticated() as u8) << 6;
        buffer.push(encrypted | authenticated | u8::from(message.ty));

        // TODO: support OEM IANA and OEM payload ID? Ignore for now: unsupported payload type

        buffer.extend_from_slice(&message.session_id.to_le_bytes());
        buffer.extend_from_slice(&message.session_sequence_number.to_le_bytes());

        let data = &message.payload;

        let data_len = data.len();

        if data_len > u16::MAX as usize {
            return Err(WriteError::PayloadTooLong);
        }

        // Confidentiality header

        // Length
        buffer.extend_from_slice(&(data_len as u16).to_le_bytes());

        // Data
        buffer.extend(data);

        // Confidentiality trailer

        // IPMI Session Trailer is only present if packets are authenticated.
        if self.authenticated() {
            // + 2 because pad data and pad length are also covered by
            // integrity checksum.
            let auth_code_data_len = buffer[4..].len() + 2;

            // Integrity PAD
            let pad_length = (4 - auth_code_data_len % 4) % 4;

            buffer.extend(std::iter::repeat(0xFF).take(pad_length));

            // Pad length
            buffer.push(pad_length as u8);

            // Next header
            buffer.push(0x07);

            // AuthCode
            let auth_code_data = &buffer[4..];

            match self.integrity_algorithm {
                IntegrityAlgorithm::None => {}
                IntegrityAlgorithm::HmacSha1_96 => {
                    let integrity_data = RunningHmac::new(&self.keys.as_ref().unwrap().k1)
                        .feed(auth_code_data)
                        .finalize();

                    buffer.extend_from_slice(&integrity_data[..12]);
                }
                IntegrityAlgorithm::HmacMd5_128 => todo!(),
                IntegrityAlgorithm::Md5_128 => todo!(),
                IntegrityAlgorithm::HmacSha256_128 => todo!(),
            };
        }

        Ok(())
    }
}
