use ml_dsa::signature::Signer;
use zeroize::{Zeroize, Zeroizing};

pub type SignerFn = Box<dyn Fn(&[u8]) -> Vec<u8>>;

pub fn make_signer_from_secret_bytes(sk_bytes: &[u8]) -> Option<SignerFn> {
    if let Ok(seed_arr) = ml_dsa::Seed::try_from(sk_bytes) {
        let seed_arr = Zeroizing::new(seed_arr);
        let sk = ml_dsa::SigningKey::<ml_dsa::MlDsa65>::from_seed(&seed_arr);
        return Some(Box::new(move |msg: &[u8]| -> Vec<u8> {
            let sig = sk.sign(msg);
            let mut encoded = sig.encode();
            let v = encoded.to_vec();
            encoded.zeroize();
            v
        }));
    }

    if let Ok(sk_arr) = <ml_dsa::ExpandedSigningKeyBytes<ml_dsa::MlDsa65>>::try_from(sk_bytes) {
        #[allow(deprecated)]
        let esk = ml_dsa::ExpandedSigningKey::<ml_dsa::MlDsa65>::from_expanded(&sk_arr);
        return Some(Box::new(move |msg: &[u8]| -> Vec<u8> {
            let sig = esk.sign(msg);
            let mut encoded = sig.encode();
            let v = encoded.to_vec();
            encoded.zeroize();
            v
        }));
    }

    None
}
