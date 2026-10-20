//! `Services`: a typed registry through which a running plugin hands a public API to the plugins that declared it as
//! a dependency (and to core consumers). A service exists while its lease does (RAII).

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

type Entry = (u64, Arc<dyn Any + Send + Sync>);

#[derive(Default)]
pub struct Services {
    map: Mutex<HashMap<TypeId, Entry>>,
    next: AtomicU64,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services").finish_non_exhaustive()
    }
}

impl Services {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Offers `value` as the service of type `T` until the lease is dropped (a newer offer of the same type
    /// replaces it; the older lease's drop leaves the newer one alone).
    #[must_use]
    pub fn provide<T: Any + Send + Sync>(self: &Arc<Self>, value: Arc<T>) -> ServiceLease {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.map
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(TypeId::of::<T>(), (id, value));
        ServiceLease {
            services: Arc::downgrade(self),
            type_id: TypeId::of::<T>(),
            id,
        }
    }

    /// The service of type `T`, if one is offered.
    #[must_use]
    pub fn get<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        let entry = self
            .map
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&TypeId::of::<T>())
            .map(|(_, value)| Arc::clone(value))?;
        entry.downcast::<T>().ok()
    }
}

/// Teardown symmetry: the service is withdrawn when this goes.
#[derive(Debug)]
pub struct ServiceLease {
    services: Weak<Services>,
    type_id: TypeId,
    id: u64,
}

impl Drop for ServiceLease {
    fn drop(&mut self) {
        if let Some(services) = self.services.upgrade() {
            let mut map = services.map.lock().unwrap_or_else(PoisonError::into_inner);
            if map.get(&self.type_id).is_some_and(|(id, _)| *id == self.id) {
                map.remove(&self.type_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Api(u8);

    #[test]
    fn a_service_is_there_while_its_lease_is() {
        let services = Services::new();
        assert!(services.get::<Api>().is_none());
        let lease = services.provide(Arc::new(Api(7)));
        assert_eq!(services.get::<Api>().as_deref(), Some(&Api(7)));
        drop(lease);
        assert!(services.get::<Api>().is_none());
    }

    #[test]
    fn a_newer_offer_replaces_and_survives_the_older_lease() {
        let services = Services::new();
        let old = services.provide(Arc::new(Api(1)));
        let _new = services.provide(Arc::new(Api(2)));
        drop(old);
        assert_eq!(services.get::<Api>().as_deref(), Some(&Api(2)));
    }

    #[test]
    fn leases_outliving_the_registry_drop_quietly() {
        let services = Services::new();
        let lease = services.provide(Arc::new(Api(1)));
        drop(services);
        drop(lease);
    }
}
