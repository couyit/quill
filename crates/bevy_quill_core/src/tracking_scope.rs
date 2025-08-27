use std::{
    any::Any,
    sync::{atomic::AtomicBool, Arc},
};

use bevy::{
    ecs::{
        component::{ComponentId, Tick},
        world::DeferredWorld,
    },
    platform::collections::HashSet,
    prelude::*,
};

use crate::{AnyCallback, UnregisterCallbackCmd};

/// Tracks the sequence of hook calls within a reaction.
#[derive(Clone)]
pub(crate) enum HookState {
    Entity(Entity),
    Mutable(Entity, ComponentId),
    Callback(Arc<dyn AnyCallback + Send + Sync>),
    Effect(Arc<dyn Any + Send + Sync + 'static>),
    Memo(Arc<dyn Any + Send + Sync + 'static>),
    Observer(Entity, Entity, Arc<dyn Any + Send + Sync + 'static>),
}

/// A component that tracks the dependencies of a reactive task.
#[derive(Component)]
pub struct TrackingScope {
    /// List of entities that are owned by this scope.
    hook_states: Vec<HookState>,

    /// During rebuilds, the index of the hook that is currently being processed.
    next_hook_index: usize,

    /// Set of components that we are currently subscribed to.
    component_deps: HashSet<(Entity, ComponentId, bool)>,

    /// Set of resources that we are currently subscribed to.
    resource_deps: HashSet<ComponentId>,

    /// Allows a tracking scope to be explictly marked as changed for reasons other than
    /// a component or resource dependency mutation.
    changed: AtomicBool,

    /// Engine tick used for determining if components have changed. This represents the
    /// time of the previous reaction.
    pub(crate) tick: Tick,

    /// List of cleanup functions to call when the scope is dropped.
    #[allow(clippy::type_complexity)]
    pub(crate) cleanups: Vec<Box<dyn FnOnce(&mut DeferredWorld) + 'static + Sync + Send>>,
}

/// A resource which, if inserted, displays the view entities that have reacted this frame.
#[derive(Resource)]
pub struct TrackingScopeTracing(pub Vec<Entity>);

impl FromWorld for TrackingScopeTracing {
    fn from_world(_world: &mut World) -> Self {
        Self(Vec::new())
    }
}

impl TrackingScope {
    /// Create a new tracking scope.
    pub fn new(tick: Tick) -> Self {
        Self {
            hook_states: Vec::new(),
            next_hook_index: 0,
            component_deps: HashSet::default(),
            resource_deps: HashSet::default(),
            changed: AtomicBool::new(false),
            tick,
            cleanups: Vec::new(),
        }
    }

    pub(crate) fn replace_hook(&mut self, hook: HookState) {
        assert!(self.next_hook_index <= self.hook_states.len());
        assert!(self.next_hook_index > 0);
        self.hook_states[self.next_hook_index - 1] = hook;
    }

    pub(crate) fn push_hook(&mut self, hook: HookState) {
        assert!(self.next_hook_index == self.hook_states.len());
        self.hook_states.push(hook);
        self.next_hook_index += 1;
    }

    pub(crate) fn next_hook(&mut self) -> Option<HookState> {
        if self.next_hook_index < self.hook_states.len() {
            let hook = self.hook_states[self.next_hook_index].clone();
            self.next_hook_index += 1;
            Some(hook)
        } else {
            None
        }
    }

    /// Add a cleanup function which will be run once before the next reaction.
    pub(crate) fn add_cleanup(
        &mut self,
        cleanup: impl FnOnce(&mut DeferredWorld) + 'static + Sync + Send,
    ) {
        self.cleanups.push(Box::new(cleanup));
    }

    /// Convenience method for adding a resource dependency.
    pub(crate) fn track_resource<T: Resource>(&mut self, world: &World) {
        self.resource_deps.insert(
            world
                .components()
                .resource_id::<T>()
                .unwrap_or_else(|| panic!("Unknown resource type: {}", std::any::type_name::<T>())),
        );
    }

    /// Convenience method for adding a component dependency.
    // pub(crate) fn track_component<C: Component>(&mut self, entity: Entity, world: &World) {
    //     self.track_component_id(
    //         entity,
    //         world.components().component_id::<C>().unwrap_or_else(|| {
    //             panic!("Unknown component type: {}", std::any::type_name::<C>())
    //         }),
    //     );
    // }

    /// Convenience method for adding a component dependency by component id.
    pub(crate) fn track_component_id(
        &mut self,
        entity: Entity,
        component: ComponentId,
        exists: bool,
    ) {
        self.component_deps.insert((entity, component, exists));
    }

    /// Mark the scope as changed for reasons other than a component or resource dependency.
    pub(crate) fn set_changed(&self) {
        self.changed
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Returns true if any of the dependencies of this scope have been updated since
    /// the previous reaction.
    pub(crate) fn dependencies_changed(&self, world: &World, tick: Tick) -> bool {
        self.components_changed(world, tick)
            || self.resources_changed(world, tick)
            || self.changed.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn components_changed(&self, world: &World, tick: Tick) -> bool {
        self.component_deps.iter().any(|(e, c, exists)| {
            world.get_entity(*e).map_or(false, |e| {
                e.get_change_ticks_by_id(*c)
                    .map(|ct| ct.is_changed(self.tick, tick))
                    .unwrap_or(false)
                    || *exists && e.get_by_id(*c).is_err()
            })
        })
    }

    fn resources_changed(&self, world: &World, tick: Tick) -> bool {
        self.resource_deps.iter().any(|c| {
            world
                .get_resource_change_ticks_by_id(*c)
                .map(|ct| ct.is_changed(self.tick, tick))
                .unwrap_or(false)
        })
    }

    /// Take the dependencies from another scope. Typically the other scope is a temporary
    /// scope that is used to compute the next set of dependencies.
    pub(crate) fn take_deps(&mut self, other: &mut Self) {
        self.component_deps = std::mem::take(&mut other.component_deps);
        self.resource_deps = std::mem::take(&mut other.resource_deps);
        self.cleanups = std::mem::take(&mut other.cleanups);
        self.hook_states = std::mem::take(&mut other.hook_states);
        self.changed.store(
            other.changed.load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    pub(crate) fn take_hooks(&mut self, other: &mut Self) {
        self.hook_states = std::mem::take(&mut other.hook_states)
    }
}

struct DespawnEntityCmd(Entity);

impl Command for DespawnEntityCmd {
    fn apply(self, world: &mut World) {
        world.despawn(self.0);
    }
}

pub(crate) fn cleanup_tracking_scopes(world: &mut World) {
    world
        .register_component_hooks::<TrackingScope>()
        .on_remove(|mut world, context| {
            let mut scope = world.get_mut::<TrackingScope>(context.entity).unwrap();
            let mut cleanups = std::mem::take(&mut scope.cleanups);
            let mut hooks = std::mem::take(&mut scope.hook_states);
            for cleanup_fn in cleanups.drain(..) {
                cleanup_fn(&mut world);
            }
            for hook in hooks.drain(..).rev() {
                match hook {
                    HookState::Entity(ent) => {
                        world.commands().queue(DespawnEntityCmd(ent));
                    }
                    HookState::Mutable(mutable_ent, _) => {
                        world.commands().queue(DespawnEntityCmd(mutable_ent));
                    }
                    HookState::Observer(ent, _, _) => {
                        world.commands().queue(DespawnEntityCmd(ent));
                    }
                    HookState::Callback(callback) => {
                        world.commands().queue(UnregisterCallbackCmd(callback));
                    }
                    HookState::Effect(_) | HookState::Memo(_) => {
                        // Nothing to do
                    }
                }
            }
        });
}

/// A command that triggers a reaction on a scope entity.
pub struct TriggerReaction(pub Entity);

impl Command for TriggerReaction {
    fn apply(self, world: &mut World) {
        if let Ok(mut scope_ent) = world.get_entity_mut(self.0) {
            if let Some(scope) = scope_ent.get_mut::<TrackingScope>() {
                TrackingScope::set_changed(&scope);
            } else {
                warn!("No tracking scope found for entity {:?}", self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct TestResource(bool);

    #[test]
    fn test_resource_deps_changed() {
        let mut world = World::default();
        let tick = world.change_tick();
        let mut scope = TrackingScope::new(tick);

        // No dependencies, so the result should be false
        assert!(!scope.dependencies_changed(&world, tick));

        world.increment_change_tick();
        world.insert_resource(TestResource(false));
        scope.track_resource::<TestResource>(&world);
        assert!(scope.resource_deps.len() == 1);

        // Resource added
        let tick = world.change_tick();
        assert!(scope.dependencies_changed(&world, tick));

        // Reset scope tick
        scope.tick = tick;
        assert!(!scope.dependencies_changed(&world, tick));

        // Mutate the resource
        world.increment_change_tick();
        world.get_resource_mut::<TestResource>().unwrap().0 = true;
        let tick = world.change_tick();
        assert!(scope.dependencies_changed(&world, tick));
    }
}
