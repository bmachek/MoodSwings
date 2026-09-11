//! Stable identities for the crowd.
//!
//! A visible pedestrian is deliberately streamed, but the person must not be.
//! This registry holds the small, immutable part of somebody that can safely
//! survive a despawn today: their cast role, age, disposition, voice, walking
//! pace and outfit. Activities, relationships and offscreen locations belong
//! here later, once the city has destinations to persist them against.
//!
//! The registry only reactivates an inactive profile of the role the spawner
//! already selected. The spawn streams still consume every draw regardless of
//! whether an old resident returns, so adding a resident does not reshuffle the
//! city's next anonymous arrival.

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;

use super::archetype::{AgeClass, Archetype};
use crate::mood::feeling::Temperament;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CitizenId(pub u32);

#[derive(Debug, Clone, Copy)]
pub struct ResidentProfile {
    pub archetype: Archetype,
    pub age: AgeClass,
    pub temperament: Temperament,
    pub pitch: f32,
    pub pace: f32,
    pub outfit: usize,
    pub mood: f32,
}

impl ResidentProfile {
    fn is_available_at(self, candidate: Self, dark: bool) -> bool {
        self.archetype == candidate.archetype
            && self.age == candidate.age
            && !(dark && (self.age == AgeClass::Child || self.archetype == Archetype::Missionary))
    }
}

/// The resident population exists independently of the renderer. `active`
/// maps a currently visible body back to its person, while `inactive` gives a
/// streamed-out resident first refusal on an appropriate next arrival.
#[derive(Resource, Debug, Default)]
pub struct Residents {
    next: u32,
    profiles: HashMap<CitizenId, ResidentProfile>,
    active: HashMap<CitizenId, Entity>,
    inactive: VecDeque<CitizenId>,
}

impl Residents {
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn total_count(&self) -> usize {
        self.profiles.len()
    }

    /// Reactivates a compatible person, or records a newly seen person.
    pub fn activate(
        &mut self,
        entity: Entity,
        candidate: ResidentProfile,
        dark: bool,
    ) -> (CitizenId, ResidentProfile) {
        let Some(index) = self.inactive.iter().position(|id| {
            self.profiles
                .get(id)
                .is_some_and(|profile| profile.is_available_at(candidate, dark))
        }) else {
            let id = CitizenId(self.next);
            self.next = self.next.checked_add(1).expect("resident id overflow");
            self.profiles.insert(id, candidate);
            self.active.insert(id, entity);
            return (id, candidate);
        };
        let id = self
            .inactive
            .remove(index)
            .expect("profile index was found");
        let profile = *self.profiles.get(&id).expect("inactive profile exists");
        self.active.insert(id, entity);
        (id, profile)
    }

    /// Makes absent bodies available again. This is reconciled from the ECS
    /// rather than patched into every despawn site, so shop entry and future
    /// systems cannot forget to return a resident to the population.
    pub fn reconcile(&mut self, visible: impl Iterator<Item = (Entity, CitizenId, Option<f32>)>) {
        let visible: HashMap<_, _> = visible
            .map(|(entity, id, mood)| {
                if let Some(profile) = self.profiles.get_mut(&id)
                    && let Some(mood) = mood
                {
                    profile.mood = mood.clamp(-1.0, 1.0);
                }
                (id, entity)
            })
            .collect();
        let missing: Vec<_> = self
            .active
            .keys()
            .copied()
            .filter(|id| !visible.contains_key(id))
            .collect();
        for id in missing {
            self.active.remove(&id);
            if !self.inactive.contains(&id) {
                self.inactive.push_back(id);
            }
        }
        for (id, entity) in visible {
            self.active.insert(id, entity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(archetype: Archetype, age: AgeClass) -> ResidentProfile {
        ResidentProfile {
            archetype,
            age,
            temperament: Temperament::ordinary(),
            pitch: 1.0,
            pace: 1.5,
            outfit: 0,
            mood: 0.0,
        }
    }

    #[test]
    fn a_streamed_out_resident_returns_with_the_same_identity() {
        let mut residents = Residents::default();
        let first = Entity::from_raw_u32(1).unwrap();
        let second = Entity::from_raw_u32(2).unwrap();
        let (id, original) =
            residents.activate(first, profile(Archetype::Punk, AgeClass::Adult), false);
        residents.reconcile([(first, id, Some(0.72))].into_iter());
        residents.reconcile(std::iter::empty());
        let (returned, restored) =
            residents.activate(second, profile(Archetype::Punk, AgeClass::Adult), false);
        assert_eq!(returned, id);
        assert_eq!(restored.pitch, original.pitch);
        assert!((restored.mood - 0.72).abs() < f32::EPSILON);
        assert_eq!(residents.total_count(), 1);
    }

    #[test]
    fn a_resident_is_not_active_in_two_bodies() {
        let mut residents = Residents::default();
        let first = Entity::from_raw_u32(1).unwrap();
        let second = Entity::from_raw_u32(2).unwrap();
        residents.activate(first, profile(Archetype::Punk, AgeClass::Adult), false);
        residents.activate(second, profile(Archetype::Punk, AgeClass::Adult), false);
        assert_eq!(residents.total_count(), 2);
        assert_eq!(residents.active_count(), 2);
    }

    #[test]
    fn a_child_profile_stays_inactive_after_dark() {
        let mut residents = Residents::default();
        let first = Entity::from_raw_u32(1).unwrap();
        let second = Entity::from_raw_u32(2).unwrap();
        residents.activate(first, profile(Archetype::Everyday, AgeClass::Child), false);
        residents.reconcile(std::iter::empty());
        let (id, _) =
            residents.activate(second, profile(Archetype::Everyday, AgeClass::Adult), true);
        assert_eq!(id, CitizenId(1));
        assert_eq!(residents.total_count(), 2);
    }
}
