use std::sync::Arc;

use futures::Future;
use futures::future::FutureResult;
use futures_cpupool::{CpuPool, CpuFuture};

use hash::Blake2bHash;
use bls::bls12_381::AggregatePublicKey;

use crate::multisig::{Signature, IndividualSignature, MultiSignature};
use crate::identity::IdentityRegistry;



#[derive(Clone, Debug, Fail, PartialEq, Eq)]
pub enum VerificationError {
    #[fail(display = "Signed by unknown signer: {}", signer)]
    UnknownSigner { signer: usize },
    #[fail(display = "Forged signature")]
    Forged,
}


/// Trait for a signature verification backend
pub trait Verifier {
    type Output: Future<Item=(), Error=VerificationError>;

    fn verify(&self, signature: &Signature) -> Self::Output;
}


/// A dummy verifier that accepts all signatures
pub struct DummyVerifier();

impl Verifier for DummyVerifier {
    type Output = FutureResult<(), VerificationError>;

    fn verify(&self, _signature: &Signature) -> Self::Output {
        Ok(()).into()
    }
}



pub struct MultithreadedVerifier<I: IdentityRegistry> {
    message_hash: Blake2bHash,
    identity_registry: Arc<I>,
    workers: CpuPool,
}

impl<I: IdentityRegistry> MultithreadedVerifier<I> {
    pub fn new(message_hash: Blake2bHash, identity_registry: Arc<I>, num_workers: Option<usize>) -> Self {
        let workers = if let Some(n) = num_workers {
            CpuPool::new(n)
        } else {
            CpuPool::new_num_cpus()
        };

        Self {
            message_hash,
            identity_registry,
            workers,
        }
    }

    fn verify_individual(identity_registry: Arc<I>, message_hash: Blake2bHash, individual: &IndividualSignature) -> Result<(), VerificationError> {
        if let Some(public_key) = identity_registry.public_key(individual.signer) {
            if public_key.verify_hash(message_hash, &individual.signature) {
                Ok(())
            }
            else {
                Err(VerificationError::Forged)
            }
        }
        else {
            Err(VerificationError::UnknownSigner { signer: individual.signer })
        }
    }

    fn verify_multisig(identity_registry: Arc<I>, message_hash: Blake2bHash, multisig: &MultiSignature) -> Result<(), VerificationError> {
        let mut aggregated_public_key = AggregatePublicKey::new();
        for signer in multisig.signers.iter() {
            if let Some(public_key) = identity_registry.public_key(signer) {
                aggregated_public_key.aggregate(&public_key);
            }
            else {
                return Err(VerificationError::UnknownSigner { signer })
            }
        }

        if aggregated_public_key.verify_hash(message_hash, &multisig.signature) {
            Ok(())
        }
        else {
            Err(VerificationError::Forged)
        }
    }
}

impl<I: IdentityRegistry + Sync + Send + 'static> Verifier for MultithreadedVerifier<I> {
    type Output = CpuFuture<(), VerificationError>;

    fn verify(&self, signature: &Signature) -> Self::Output {
        // We clone it so that we can move it into the closure
        let signature = signature.clone();
        let message_hash = self.message_hash.clone();
        let identity_registry = Arc::clone(&self.identity_registry);

        self.workers.spawn_fn(move || {
            match signature {
                Signature::Individual(ref individual) => {
                    Self::verify_individual(identity_registry, message_hash, individual)
                },
                Signature::Multi(ref multisig) => {
                    Self::verify_multisig(identity_registry, message_hash, multisig)
                }
            }
        })
    }
}