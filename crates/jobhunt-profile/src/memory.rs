//! In-memory [`ProfileRepository`] for domain tests.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::aggregate::ProfileData;
use crate::evidence::{Claim, ClaimQuery};
use crate::ids::ProfileId;
use crate::repository::{ProfileEvent, ProfileRepository, StorageError};

#[derive(Default)]
struct State {
    profiles: HashMap<ProfileId, ProfileData>,
    events: Vec<(ProfileId, ProfileEvent)>,
}

#[derive(Default)]
pub struct MemoryProfiles {
    state: Mutex<State>,
}

impl MemoryProfiles {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[async_trait]
impl ProfileRepository for MemoryProfiles {
    async fn load_profile(&self, id: ProfileId) -> Result<Option<ProfileData>, StorageError> {
        Ok(self.lock().profiles.get(&id).cloned())
    }

    async fn save_profile(
        &self,
        data: &ProfileData,
        expected_revision: u64,
        events: &[ProfileEvent],
    ) -> Result<(), StorageError> {
        let mut state = self.lock();
        let found = state
            .profiles
            .get(&data.id())
            .map_or(0, |d| d.profile.revision);
        if found != expected_revision {
            return Err(StorageError::Conflict {
                expected: expected_revision,
                found,
            });
        }
        state.profiles.insert(data.id(), data.clone());
        state
            .events
            .extend(events.iter().map(|e| (data.id(), e.clone())));
        Ok(())
    }

    async fn find_claims(
        &self,
        profile: ProfileId,
        query: &ClaimQuery,
    ) -> Result<Vec<Claim>, StorageError> {
        Ok(self
            .lock()
            .profiles
            .get(&profile)
            .map(|d| d.claims(query).into_iter().cloned().collect())
            .unwrap_or_default())
    }

    async fn profile_events(
        &self,
        profile: ProfileId,
        limit: usize,
    ) -> Result<Vec<ProfileEvent>, StorageError> {
        Ok(self
            .lock()
            .events
            .iter()
            .rev()
            .filter(|(p, _)| *p == profile)
            .take(limit)
            .map(|(_, e)| e.clone())
            .collect())
    }
}
