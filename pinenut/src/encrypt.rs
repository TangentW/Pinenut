//! Encryption & Decryption.

use thiserror::Error;

use crate::Sealed;

/// Errors that can be occurred during encryption or decryption.
#[derive(Error, Clone, Debug)]
pub enum Error {
    /// An error that occurs during padding or unpadding.
    #[error("padding error")]
    Padding,
    /// An error that occurs during ECDH.
    #[error("ECDH error")]
    Ecdh,
}

/// Errors that can be occurred during encryption.
pub type EncryptionError = Error;

/// Errors that can be occurred during decryption.
pub type DecryptionError = Error;

/// Represents the type of encryption keys.
///
/// `Pinenut` uses encryption keys of length 16 bytes (128 bits).
pub type EncryptionKey = [u8; 16];

/// Represents the length of the public key.
///
/// A public key is a compressed elliptic curve point.
/// With length: 1 byte (encoding tag) + 32 bytes (256 bits).
pub const PUBLIC_KEY_LEN: usize = 33;

/// Operation of encryption. Different values are used according to different flush
/// dimensions.
#[derive(Debug, Clone, Copy)]
pub(crate) enum EncryptOp<'a> {
    Input(&'a [u8]),
    Flush,
}

/// Represents a target for encrypted data or decrypted data.
pub(crate) trait Sink = crate::Sink<Error>;

/// Represents a block encryptor that encrypts data to its target (`Sink`).
pub(crate) trait Encryptor: Sealed {
    fn encrypt<S>(&mut self, operation: EncryptOp, sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink;
}

/// Represents a block decryptor that decrypts data to its target (`Sink`).
pub(crate) trait Decryptor: Sealed {
    fn decrypt<S>(
        &mut self,
        input: &[u8],
        reached_to_end: bool,
        sink: &mut S,
    ) -> Result<(), S::Error>
    where
        S: Sink;
}

pub use ecdh::{gen_echd_key_pair, PublicKey, SecretKey};

/// `Elliptic Curve Diffie–Hellman (ECDH)` Support.
///
/// Using the NIST P-256 (a.k.a. secp256r1, prime256v1) elliptic curve.
pub(crate) mod ecdh {
    use std::mem;

    use p256::{ecdh::diffie_hellman, elliptic_curve};
    use rand_core::OsRng;

    use crate::encrypt::{EncryptionKey, Error, PUBLIC_KEY_LEN};

    /// Represents the type of secret keys.
    ///
    /// With length: 32 bytes (256 bits).
    pub type SecretKey = [u8; 32];

    /// Represents the type of public keys.
    ///
    /// A public key is a compressed elliptic curve point.
    /// With length: 1 byte (encoding tag) + 32 bytes (256 bits).
    pub type PublicKey = [u8; 33];

    /// The empty public key, it means no encryption.
    pub(crate) const EMPTY_PUBLIC_KEY: PublicKey = [0; PUBLIC_KEY_LEN];

    impl From<elliptic_curve::Error> for Error {
        #[inline]
        fn from(_: elliptic_curve::Error) -> Self {
            Self::Ecdh
        }
    }

    /// Generates the ECHD key pair.
    #[inline]
    pub fn gen_echd_key_pair() -> (SecretKey, PublicKey) {
        let secret_key = p256::SecretKey::random(&mut OsRng);
        let public_key = p256::EncodedPoint::from(secret_key.public_key()).compress();
        (secret_key.to_bytes().into(), public_key.as_bytes().try_into().unwrap())
    }

    /// Represents the public and symmetric keys generated in the initialization of
    /// the logger.
    pub(crate) struct Keys {
        /// Represents the public key passed to the consumer (viewer).
        pub(crate) public_key: PublicKey,
        /// Represents the symmetric key during log record encryption.
        pub(crate) encryption_key: EncryptionKey,
    }

    impl Keys {
        /// Constructs the `Keys` via Elliptic Curve Diffie-Hellman (ECDH).
        pub(crate) fn new(public_key: &PublicKey) -> Result<Self, Error> {
            let public_key = p256::PublicKey::from_sec1_bytes(public_key.as_ref())?;
            let secret_key = p256::SecretKey::random(&mut OsRng);

            let encryption_key =
                diffie_hellman(secret_key.to_nonzero_scalar(), public_key.as_affine());
            let encryption_key = encryption_key.raw_secret_bytes().as_slice()
                [..mem::size_of::<EncryptionKey>()]
                .try_into()
                .map_err(|_| Error::Ecdh)?;

            let public_key = p256::EncodedPoint::from(secret_key.public_key()).compress();
            let public_key = public_key.as_bytes().try_into().map_err(|_| Error::Ecdh)?;

            Ok(Self { public_key, encryption_key })
        }
    }

    /// Negotiates the symmetric key during log record encryption via Elliptic Curve
    /// Diffie-Hellman (ECDH).
    #[inline]
    pub(crate) fn ecdh_encryption_key(
        secret_key: &SecretKey,
        public_key: &PublicKey,
    ) -> Result<EncryptionKey, Error> {
        let secret_key = p256::SecretKey::from_slice(secret_key.as_ref())?;
        let public_key = p256::PublicKey::from_sec1_bytes(public_key.as_ref())?;

        let encryption_key = diffie_hellman(secret_key.to_nonzero_scalar(), public_key.as_affine());
        encryption_key.raw_secret_bytes().as_slice()[..mem::size_of::<EncryptionKey>()]
            .try_into()
            .map_err(|_| Error::Ecdh)
    }
}

pub(crate) use aes::{Decryptor as AesDecryptor, Encryptor as AesEncryptor};

/// `Encryptor` and `Decryptor` for the `AES 128` encryption, with `ECB` mode and
/// `PKCS#7` padding.
///
/// In this situation, we consider using `ECB` mode to trade off between performance
/// and security. Because the bytes of data are compressed before they are encrypted.
pub(crate) mod aes {
    use aes::{Aes128Dec, Aes128Enc};
    use cipher::{
        block_padding::{NoPadding, Pkcs7, UnpadError},
        inout::PadError,
        BlockDecrypt, BlockEncrypt, KeyInit,
    };

    use crate::{
        common::BytesBuf,
        encrypt::{
            Decryptor as DecryptorTrait, EncryptOp, EncryptionKey, Encryptor as EncryptorTrait,
            Error, Sink,
        },
        Sealed,
    };

    /// 128-bit AES block.
    const BLOCK_SIZE: usize = 16;

    impl From<PadError> for Error {
        #[inline]
        fn from(_: PadError) -> Self {
            Self::Padding
        }
    }

    impl From<UnpadError> for Error {
        #[inline]
        fn from(_: UnpadError) -> Self {
            Self::Padding
        }
    }

    /// The `AES` encryptor.
    pub(crate) struct Encryptor {
        inner: Aes128Enc,
        buffer: BytesBuf,
    }

    impl Encryptor {
        /// Should not be less than `BLOCK_SIZE`.
        ///
        /// Buffer of 256 bytes should be sufficient for encryption of a log.
        const BUFFER_LEN: usize = 16 * BLOCK_SIZE;

        /// Constructs a new `Encryptor` with encryption key.
        #[inline]
        pub(crate) fn new(key: &EncryptionKey) -> Self {
            let inner = Aes128Enc::new(key.into());
            let buffer = BytesBuf::with_capacity(Self::BUFFER_LEN);
            Self { inner, buffer }
        }
    }

    impl EncryptorTrait for Encryptor {
        fn encrypt<S>(&mut self, operation: EncryptOp, sink: &mut S) -> Result<(), S::Error>
        where
            S: Sink,
        {
            match operation {
                EncryptOp::Input(mut input) => {
                    while !input.is_empty() {
                        let buffered = self.buffer.buffer(input);
                        debug_assert_ne!(
                            buffered, 0,
                            "the size of buffer needs to be greater than or equal to `BLOCK_SIZE`"
                        );

                        self.buffer.sink(sink, false, |buf, len| {
                            self.inner.encrypt_padded::<NoPadding>(buf, len)
                        })?;

                        // The remaining input.
                        input = &input[buffered..];
                    }
                    Ok(())
                }
                EncryptOp::Flush => self
                    .buffer
                    .sink(sink, true, |buf, len| self.inner.encrypt_padded::<Pkcs7>(buf, len)),
            }
        }
    }

    impl Sealed for Encryptor {}

    /// The `AES` decryptor.
    pub(crate) struct Decryptor {
        inner: Aes128Dec,
        buffer: BytesBuf,
    }

    impl Decryptor {
        /// Should not be less than `BLOCK_SIZE`.
        ///
        /// Uses 1KB as the buffer length for decryption.
        const BUFFER_LEN: usize = 64 * BLOCK_SIZE;

        /// Constructs a new `Decryptor` with encryption key.
        #[inline]
        pub(crate) fn new(key: &EncryptionKey) -> Self {
            let inner = Aes128Dec::new(key.into());
            let buffer = BytesBuf::with_capacity(Self::BUFFER_LEN);
            Self { inner, buffer }
        }
    }

    impl DecryptorTrait for Decryptor {
        fn decrypt<S>(
            &mut self,
            mut input: &[u8],
            reached_to_end: bool,
            sink: &mut S,
        ) -> Result<(), S::Error>
        where
            S: Sink,
        {
            while !input.is_empty() {
                let buffered = self.buffer.buffer(input);
                debug_assert_ne!(
                    buffered, 0,
                    "the size of buffer needs to be greater than or equal to `BLOCK_SIZE`"
                );

                let reached_to_end = reached_to_end && buffered == input.len();
                self.buffer.sink(sink, reached_to_end, |buf, len| {
                    let buf = &mut buf[..len];
                    if reached_to_end {
                        self.inner.decrypt_padded::<Pkcs7>(buf)
                    } else {
                        self.inner.decrypt_padded::<NoPadding>(buf)
                    }
                })?;

                // The remaining input.
                input = &input[buffered..];
            }
            Ok(())
        }
    }

    impl Sealed for Decryptor {}

    impl BytesBuf {
        /// Handle the bytes of specified length, then writes them to the sink, and
        /// finally drains the buffer.
        fn sink<S, E>(
            &mut self,
            sink: &mut S,
            pad: bool,
            handle: impl FnOnce(&mut [u8], usize) -> Result<&[u8], E>,
        ) -> Result<(), S::Error>
        where
            S: Sink,
            E: Into<Error>,
        {
            let len = if pad { self.len() } else { self.len() / BLOCK_SIZE * BLOCK_SIZE };
            let buffer = self.as_buffer_mut_slice();

            let bytes = handle(buffer, len).map_err(Into::into)?;
            if !bytes.is_empty() {
                sink.sink(bytes)?;
            }
            self.drain(len);

            Ok(())
        }
    }
}

impl<T> Encryptor for Option<T>
where
    T: Encryptor,
{
    #[inline]
    fn encrypt<S>(&mut self, operation: EncryptOp, sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink,
    {
        match self {
            Some(encryptor) => encryptor.encrypt(operation, sink),
            // Just writes its all input to the sink directly.
            None => match operation {
                EncryptOp::Input(bytes) => sink.sink(bytes),
                _ => Ok(()),
            },
        }
    }
}

impl<T> Decryptor for Option<T>
where
    T: Decryptor,
{
    #[inline]
    fn decrypt<S>(
        &mut self,
        input: &[u8],
        reached_to_end: bool,
        sink: &mut S,
    ) -> Result<(), S::Error>
    where
        S: Sink,
    {
        match self {
            Some(decryptor) => decryptor.decrypt(input, reached_to_end, sink),
            // Just writes its all input to the sink directly.
            None => sink.sink(input),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::slice;

    use crate::encrypt::{
        AesDecryptor, AesEncryptor, Decryptor, EncryptOp, EncryptionKey, Encryptor,
    };

    const KEY: EncryptionKey = [0x23; 16];

    fn aes_encrypt(input: &[u8]) -> Vec<u8> {
        let mut encryptor = AesEncryptor::new(&KEY);
        let mut sink = Vec::new();
        let mut sink_mul = Vec::new();

        // One time.
        encryptor.encrypt(EncryptOp::Input(input), &mut sink).unwrap();
        encryptor.encrypt(EncryptOp::Flush, &mut sink).unwrap();

        // Multiple times.
        for byte in input {
            encryptor.encrypt(EncryptOp::Input(slice::from_ref(byte)), &mut sink_mul).unwrap();
        }
        encryptor.encrypt(EncryptOp::Flush, &mut sink_mul).unwrap();

        assert_eq!(sink, sink_mul);
        sink
    }

    fn aes_decrypt(input: &[u8]) -> Vec<u8> {
        let mut decryptor = AesDecryptor::new(&KEY);
        let mut sink = Vec::new();
        let mut sink_mul = Vec::new();

        // One time.
        decryptor.decrypt(input, true, &mut sink).unwrap();

        // Multiple times.
        for (idx, byte) in input.iter().enumerate() {
            decryptor
                .decrypt(slice::from_ref(byte), idx == input.len() - 1, &mut sink_mul)
                .unwrap();
        }

        assert_eq!(sink, sink_mul);
        sink
    }

    #[test]
    fn test_aes() {
        // Short data
        let data = b"Hello World";
        assert_eq!(aes_decrypt(&aes_encrypt(data)), data);

        // 16 bytes data
        let data = b"123456789ABCDEFG";
        assert_eq!(aes_decrypt(&aes_encrypt(data)), data);

        // Long data
        let data = b"Hello, I'm Tangent, nice to meet you.";
        assert_eq!(aes_decrypt(&aes_encrypt(data)), data);
    }
}
