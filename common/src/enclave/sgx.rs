//! SGX-specific implementations for in-enclave APIs

use ring::aead::{
    Aad, BoundKey, Nonce, NonceSequence, SealingKey, UnboundKey, AES_256_GCM,
    MAX_TAG_LEN,
};
use secrecy::zeroize::Zeroizing;
use sgx_isa::{AttributesFlags, Keyname, Keypolicy, Keyrequest, Report};

use crate::enclave::{Error, Sealed};
use crate::rng::Crng;

fn gen_seal_keyrequest(
    rng: &mut dyn Crng,
    label: [u8; 16],
    self_report: &Report,
) -> Keyrequest {
    // keyid := [ label || rand[0..16] ]
    let mut keyid = [0u8; 32];
    keyid[0..16].copy_from_slice(&label);
    rng.fill_bytes(&mut keyid[16..32]);

    // NOTE: default masks adapted from [openenclave/sgxtypes.h](https://github.com/openenclave/openenclave/blob/master/include/openenclave/bits/sgx/sgxtypes.h#L1097)

    // ignore reserved bits + PROVISIONKEY + EINITTOKENKEY
    let attribute_mask: u64 = !(0xffffffffffffc0
        | AttributesFlags::PROVISIONKEY.bits()
        | AttributesFlags::EINITTOKENKEY.bits());
    // ignore all
    let xfrm_mask: u64 = !0;
    // ignore all except upper byte
    let misc_mask: u32 = !0x0fffffff;

    Keyrequest {
        keyname: Keyname::Seal as _,
        keypolicy: Keypolicy::MRENCLAVE,
        isvsvn: self_report.isvsvn,
        cpusvn: self_report.cpusvn,
        attributemask: [!attribute_mask, xfrm_mask],
        miscmask: misc_mask,
        keyid,
        ..Default::default()
    }
}

/// Truncate a full 512 SGX [`Keyrequest`] to 76 bytes, leaving off the empty
/// reserved bytes. This makes our `Sealed` data significantly more compact.
fn truncated_keyrequest(keyrequest: &Keyrequest) -> &[u8] {
    let bytes: &[u8] = keyrequest.as_ref();
    &bytes[0..76]
}

/// We sample a unique sealing key per seal request. just grab the random part
/// of the label as a nonce.
fn nonce_from_keyrequest(keyrequest: &Keyrequest) -> Nonce {
    let (_, nonce) = keyrequest.keyid.rsplit_array_ref::<12>();
    Nonce::assume_unique_for_key(*nonce)
}

fn get_sealing_key(
    keyrequest: &Keyrequest,
) -> Result<SealingKey<OnlyOnce>, Error> {
    let key_material = Zeroizing::new(keyrequest.egetkey()?);
    let nonce = OnlyOnce::new(nonce_from_keyrequest(keyrequest));
    Ok(SealingKey::new(
        UnboundKey::new(&AES_256_GCM, key_material.as_ref())
            .expect("Invalid key size"),
        nonce,
    ))
}

/// A nonce wrapper that only allows a key to seal/unseal once.
struct OnlyOnce(Option<Nonce>);

impl OnlyOnce {
    fn new(nonce: Nonce) -> Self {
        Self(Some(nonce))
    }
}

impl NonceSequence for OnlyOnce {
    fn advance(&mut self) -> Result<Nonce, ring::error::Unspecified> {
        // This should never happen
        Ok(self
            .0
            .take()
            .expect("sealed / unseal more than once with the same key"))
    }
}

pub fn seal(
    rng: &mut dyn Crng,
    label: [u8; 16],
    data: &[u8],
) -> Result<Sealed<'static>, Error> {
    let self_report = Report::for_self();
    let keyrequest = gen_seal_keyrequest(rng, label, &self_report);
    let mut sealing_key = get_sealing_key(&keyrequest)?;

    let keyrequest_bytes = truncated_keyrequest(&keyrequest).to_owned();
    let attributes = self_report.attributes.flags.bits();
    let xfrm = self_report.attributes.xfrm;
    let miscselect = self_report.miscselect.bits();

    // TODO(phlip9): what to include in AAD? does it make sense to include
    // keyrequest and attributes+miscselect?

    let mut ciphertext = vec![0u8; data.len() + MAX_TAG_LEN];
    sealing_key.seal_in_place_append_tag(Aad::empty(), &mut ciphertext)?;

    Ok(Sealed {
        keyrequest: keyrequest_bytes.into(),
        attributes: [attributes, xfrm],
        miscselect,
        ciphertext: std::borrow::Cow::Borrowed(b""),
    })
}

pub fn unseal(_label: [u8; 16], _sealed: Sealed<'_>) -> Result<Vec<u8>, Error> {
    Err(Error::Other)
}
