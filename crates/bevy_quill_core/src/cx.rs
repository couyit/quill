use std::{cell::RefCell, marker::PhantomData, sync::Arc};

use bevy::{
    ecs::{
        bundle::Bundle, event::Event, observer::Observer, system::IntoObserverSystem,
        world::DeferredWorld,
    },
    hierarchy::{BuildChildren, Parent},
    prelude::{Component, Entity, IntoSystem, Resource, SystemInput, World},
};

use crate::{mutable::Mutable, tracking_scope::HookState, Callback, MutableCell, WriteMutable};
use crate::{tracking_scope::TrackingScope, ReadMutable};

#[derive(Clone)]
struct Memo<R: Clone, D: Clone> {
    result: R,
    deps: D,
}

#[derive(Copy, Clone, PartialEq)]
pub struct EffectOptions {
    /// Run the effect once when first called. Default true.
    pub run_immediately: bool,
}

impl Default for EffectOptions {
    fn default() -> Self {
        Self {
            run_immediately: true,
        }
    }
}

/// A context parameter that is passed to views and callbacks. It contains the reactive
/// tracking scope, which is used to manage reactive dependencies, as well as a reference to
/// the Bevy world.
pub struct Cx<'p, 'w> {
    /// Bevy World
    world: &'w mut World,

    /// The entity that owns the tracking scope (or will own it).
    owner: Entity,

    /// Set of reactive resources referenced by the presenter.
    pub(crate) tracking: RefCell<&'p mut TrackingScope>,
}

impl<'p, 'w> Cx<'p, 'w> {
    /// Construct a new reactive context.
    pub fn new(world: &'w mut World, owner: Entity, tracking: &'p mut TrackingScope) -> Self {
        Self {
            world,
            owner,
            tracking: RefCell::new(tracking),
        }
    }

    /// Access to world from reactive context.
    pub fn world(&self) -> &World {
        self.world
    }

    /// Access to mutable world from reactive context.
    pub fn world_mut(&mut self) -> &mut World {
        self.world
    }

    /// Returns the id of the entity that owns the tracking scope.
    pub fn owner(&self) -> Entity {
        self.owner
    }

    // Spawn an empty [`Entity`]. The caller is responsible for despawning the entity.
    // pub fn create_entity_untracked(&mut self) -> Entity {
    //     self.world_mut().spawn_empty().id()
    // }

    /// Spawn an empty [`Entity`]. The entity will be despawned when the tracking scope is dropped.
    pub fn create_entity(&mut self) -> Entity {
        let hook = self.tracking.borrow_mut().next_hook();
        match hook {
            Some(HookState::Entity(entity)) => entity,
            Some(_) => {
                panic!("Expected create_entity() hook, found something else");
            }
            None => {
                let entity = self.world_mut().spawn_empty().id();
                self.tracking
                    .borrow_mut()
                    .push_hook(HookState::Entity(entity));
                entity
            }
        }
    }

    /// Create a new [`Mutable`] in this context.
    pub fn create_mutable<T>(&mut self, init: T) -> Mutable<T>
    where
        T: Send + Sync + 'static,
    {
        let hook = self.tracking.borrow_mut().next_hook();
        match hook {
            Some(HookState::Mutable(cell, component)) => Mutable {
                cell,
                component,
                marker: PhantomData,
            },

            Some(_) => {
                panic!("Expected create_mutable() hook, found something else");
            }
            None => {
                let owner = self.owner();
                let cell = self
                    .world_mut()
                    .spawn(MutableCell::<T>(init))
                    .set_parent(owner)
                    .id();
                let component = self.world_mut().register_component::<MutableCell<T>>();
                self.tracking
                    .borrow_mut()
                    .push_hook(HookState::Mutable(cell, component));
                Mutable {
                    cell,
                    component,
                    marker: PhantomData,
                }
            }
        }
    }

    /// Create an effect which runs each time the reactive context is executed, *and* the given
    /// dependencies change.
    ///
    /// Arguments:
    /// - `effect_fn`: The effect function to run.
    /// - `deps`: The dependencies which trigger the effect.
    pub fn create_effect<
        S: Fn(&mut World, D) + Send + Sync,
        D: PartialEq + Clone + Send + Sync + 'static,
    >(
        &mut self,
        effect_fn: S,
        deps: D,
    ) {
        self.create_effect_ext(effect_fn, deps, EffectOptions::default())
    }

    /// Create an effect which runs each time the reactive context is executed, *and* the given
    /// dependencies change. This version takes additional options.
    ///
    /// Arguments:
    /// - `effect_fn`: The effect function to run.
    /// - `deps`: The dependencies which trigger the effect.
    /// - `options`: Additional options for running the effect.
    pub fn create_effect_ext<
        S: Fn(&mut World, D) + Send + Sync,
        D: PartialEq + Clone + Send + Sync + 'static,
    >(
        &mut self,
        effect_fn: S,
        deps: D,
        options: EffectOptions,
    ) {
        let hook = self.tracking.borrow_mut().next_hook();
        match hook {
            Some(HookState::Effect(prev_deps)) => match prev_deps.downcast_ref::<D>() {
                Some(prev_deps) => {
                    if *prev_deps != deps {
                        effect_fn(self.world, deps.clone());
                        self.tracking
                            .borrow_mut()
                            .replace_hook(HookState::Effect(Arc::new(deps)));
                    }
                }
                None => {
                    panic!("Effect dependencies type mismatch");
                }
            },
            Some(_) => {
                panic!("Expected create_effect() hook, found something else");
            }
            None => {
                if options.run_immediately {
                    effect_fn(self.world, deps.clone());
                }
                self.tracking
                    .borrow_mut()
                    .push_hook(HookState::Effect(Arc::new(deps)));
            }
        }
    }

    /// Create a memoized value which is only recomputed when dependencies change.
    ///
    /// Arguments:
    /// - `factory_fn`: The factory function which computes the memoized value.
    /// - `deps`: The dependencies which trigger the effect.
    pub fn create_memo<
        R: Clone + Send + Sync + 'static,
        S: Fn(&mut World, D) -> R + Send + Sync,
        D: PartialEq + Clone + Send + Sync + 'static,
    >(
        &mut self,
        factory_fn: S,
        deps: D,
    ) -> R {
        let hook = self.tracking.borrow_mut().next_hook();
        match hook {
            Some(HookState::Memo(memo)) => match memo.downcast_ref::<Memo<R, D>>() {
                Some(prev_memo) => {
                    if prev_memo.deps != deps {
                        let result = factory_fn(self.world, deps.clone());
                        self.tracking
                            .borrow_mut()
                            .replace_hook(HookState::Memo(Arc::new(Memo {
                                result: result.clone(),
                                deps,
                            })));
                        result
                    } else {
                        prev_memo.result.clone()
                    }
                }
                None => {
                    panic!("Memo dependencies type mismatch");
                }
            },
            Some(_) => {
                panic!("Expected create_memo() hook, found something else");
            }
            None => {
                let result = factory_fn(self.world, deps.clone());
                self.tracking
                    .borrow_mut()
                    .push_hook(HookState::Memo(Arc::new(Memo {
                        result: result.clone(),
                        deps,
                    })));
                result
            }
        }
    }

    pub fn create_observer<
        E: Event,
        B: Bundle,
        M,
        I: IntoObserverSystem<E, B, M>,
        D: PartialEq + Clone + Send + Sync + 'static,
    >(
        &mut self,
        system: I,
        target: Entity,
        deps: D,
    ) -> Entity {
        let hook = self.tracking.borrow_mut().next_hook();
        match hook {
            Some(HookState::Observer(prev_observer, prev_target, prev_deps)) => {
                if prev_target == target
                    && *prev_deps
                        .downcast_ref::<D>()
                        .expect("Observer dependencies type mismatch")
                        == deps
                {
                    prev_observer
                } else {
                    self.world_mut().despawn(prev_observer);
                    let observer = self
                        .world_mut()
                        .spawn(Observer::new(system).with_entity(target))
                        .id();
                    self.tracking.borrow_mut().replace_hook(HookState::Observer(
                        observer,
                        target,
                        Arc::new(deps),
                    ));
                    observer
                }
            }
            Some(_) => {
                panic!("Expected create_observer() hook, found something else");
            }
            None => {
                let observer = self
                    .world_mut()
                    .spawn(Observer::new(system).with_entity(target))
                    .id();
                self.tracking.borrow_mut().push_hook(HookState::Observer(
                    observer,
                    target,
                    Arc::new(deps),
                ));
                observer
            }
        }
    }

    /// Create a memoized value which is only recomputed when dependencies change. This version
    /// uses a user-supplied comparison function to determine if the dependencies have changed.
    ///
    /// Arguments:
    /// - `factory_fn`: The factory function which computes the memoized value.
    /// - `deps`: The dependencies which trigger the effect.
    pub fn create_memo_cmp<
        R: Clone + Send + Sync + 'static,
        S: Fn(&mut Cx, D) -> R + Send + Sync,
        C: Fn(&D, &D) -> bool,
        D: Clone + Send + Sync + 'static,
    >(
        &mut self,
        factory_fn: S,
        cmp: C,
        deps: D,
    ) -> R {
        let hook = self.tracking.borrow_mut().next_hook();
        match hook {
            Some(HookState::Memo(memo)) => match memo.downcast_ref::<Memo<R, D>>() {
                Some(prev_memo) => {
                    if !cmp(&prev_memo.deps, &deps) {
                        let result = factory_fn(self, deps.clone());
                        self.tracking
                            .borrow_mut()
                            .replace_hook(HookState::Memo(Arc::new(Memo {
                                result: result.clone(),
                                deps,
                            })));
                        result
                    } else {
                        prev_memo.result.clone()
                    }
                }
                None => {
                    panic!("Memo dependencies type mismatch");
                }
            },
            Some(_) => {
                panic!("Expected create_memo() hook, found something else");
            }
            None => {
                let result = factory_fn(self, deps.clone());
                self.tracking
                    .borrow_mut()
                    .push_hook(HookState::Memo(Arc::new(Memo {
                        result: result.clone(),
                        deps,
                    })));
                result
            }
        }
    }

    /// Create a new callback in this context. This registers a one-shot system with the world.
    /// The callback will be unregistered when the tracking scope is dropped.
    ///
    /// Note: This function takes no deps argument, the callback is only registered once the first
    /// time it is called. Subsequent calls will return the original callback.
    pub fn create_callback<
        P: Send + Sync + SystemInput + 'static,
        M,
        S: IntoSystem<P, (), M> + 'static,
    >(
        &mut self,
        callback: S,
    ) -> Callback<P> {
        let hook = self.tracking.borrow_mut().next_hook();
        match hook {
            Some(HookState::Callback(cb)) => cb.as_ref().downcast::<P>(),
            Some(_) => {
                panic!("Expected create_callback() hook, found something else");
            }
            None => {
                let id = self.world_mut().register_system(callback);
                let result = Callback { id };
                self.tracking
                    .borrow_mut()
                    .push_hook(HookState::Callback(Arc::new(result)));
                result
            }
        }
    }

    /// Temporary hook used to create a new [`Mutable`] which is automatically updated
    /// each time this hook is called. This is used for now until we get replaceable one-shot systems.
    ///
    /// You cannot create multiple captures of the same type within a single tracking scope.
    pub fn create_capture<T>(&mut self, init: T) -> Mutable<T>
    where
        T: Clone + PartialEq + Send + Sync + 'static,
    {
        let hook = self.tracking.borrow_mut().next_hook();
        match hook {
            Some(HookState::Mutable(cell, component)) => {
                let result = Mutable {
                    cell,
                    component,
                    marker: PhantomData,
                };
                result.set_clone(self.world, init);
                result
            }

            Some(_) => {
                panic!("Expected create_mutable() hook, found something else");
            }
            None => {
                let owner = self.owner();
                let cell = self
                    .world_mut()
                    .spawn(MutableCell::<T>(init))
                    .set_parent(owner)
                    .id();
                let component = self.world_mut().register_component::<MutableCell<T>>();
                self.tracking
                    .borrow_mut()
                    .push_hook(HookState::Mutable(cell, component));
                Mutable {
                    cell,
                    component,
                    marker: PhantomData,
                }
            }
        }
    }

    /// Insert a component on the owner entity of the current context. This component can
    /// be accessed by this context any any child contexts via [`use_inherited_component`].
    pub fn insert(&mut self, component: impl Component) {
        let owner = self.owner;
        self.world_mut().entity_mut(owner).insert(component);
    }

    /// Return a reference to the resource of the given type. Calling this function
    /// adds the resource as a dependency of the current presenter invocation.
    pub fn use_resource<T: Resource>(&self) -> &T {
        self.tracking.borrow_mut().track_resource::<T>(self.world);
        self.world.resource::<T>()
    }

    /// Return a reference to the resource of the given type. Calling this function
    /// does not add the resource as a dependency of the current presenter invocation.
    pub fn use_resource_untracked<T: Resource>(&self) -> &T {
        self.world.resource::<T>()
    }

    /// Return a reference to the Component `C` on the given entity.
    pub fn use_component<C: Component>(&self, entity: Entity) -> Option<&C> {
        match self.world.get_entity(entity) {
            Ok(c) => {
                let cid = self
                    .world
                    .components()
                    .component_id::<C>()
                    .unwrap_or_else(|| {
                        panic!("Unknown component type: {}", std::any::type_name::<C>())
                    });
                let result = c.get::<C>();
                self.tracking
                    .borrow_mut()
                    .track_component_id(entity, cid, result.is_some());
                result
            }
            Err(_) => None,
        }
    }

    /// Return a reference to the Component `C` on the given entity. This version does not
    /// add the component to the tracking scope, and is intended for components that update
    /// frequently.
    pub fn use_component_untracked<C: Component>(&self, entity: Entity) -> Option<&C> {
        match self.world.get_entity(entity) {
            Ok(c) => c.get::<C>(),
            Err(_) => None,
        }
    }

    /// Return a reference to the Component `C` on the owner entity of the current
    /// context, or one of it's ancestors. This searches up the entity tree until it finds
    /// a component of the given type. If found, the component is added to the current tracking
    /// scope.
    pub fn use_inherited_component<C: Component>(&self) -> Option<&C> {
        let mut entity = self.owner;
        loop {
            let ec = self.use_component(entity);
            if ec.is_some() {
                return ec;
            }
            match self.world.entity(entity).get::<Parent>() {
                Some(parent) => entity = **parent,
                _ => return None,
            }
        }
    }

    /// Add a cleanup function which is run once before the next reaction, or when the owner
    /// entity for this context is despawned.
    pub fn on_cleanup(&mut self, cleanup: impl FnOnce(&mut DeferredWorld) + Send + Sync + 'static) {
        self.tracking.borrow_mut().add_cleanup(cleanup);
    }
}

impl<'p, 'w> ReadMutable for Cx<'p, 'w> {
    fn read_mutable<T>(&self, mutable: &Mutable<T>) -> T
    where
        T: Send + Sync + Copy + 'static,
    {
        self.tracking
            .borrow_mut()
            .track_component_id(mutable.cell, mutable.component, true);
        self.world.read_mutable(mutable)
    }

    fn read_mutable_clone<T>(&self, mutable: &Mutable<T>) -> T
    where
        T: Send + Sync + Clone + 'static,
    {
        self.tracking
            .borrow_mut()
            .track_component_id(mutable.cell, mutable.component, true);
        self.world.read_mutable_clone(mutable)
    }

    fn read_mutable_as_ref<T>(&self, mutable: &Mutable<T>) -> &T
    where
        T: Send + Sync + 'static,
    {
        self.tracking
            .borrow_mut()
            .track_component_id(mutable.cell, mutable.component, true);
        self.world.read_mutable_as_ref(mutable)
    }

    fn read_mutable_map<T, U, F: Fn(&T) -> U>(&self, mutable: &Mutable<T>, f: F) -> U
    where
        T: Send + Sync + 'static,
    {
        self.tracking
            .borrow_mut()
            .track_component_id(mutable.cell, mutable.component, true);
        self.world.read_mutable_map(mutable, f)
    }
}

impl<'p, 'w> WriteMutable for Cx<'p, 'w> {
    fn write_mutable<T>(&mut self, mutable: Entity, value: T)
    where
        T: Send + Sync + Copy + PartialEq + 'static,
    {
        self.world.write_mutable(mutable, value);
    }

    fn write_mutable_clone<T>(&mut self, mutable: Entity, value: T)
    where
        T: Send + Sync + Clone + PartialEq + 'static,
    {
        self.world.write_mutable_clone(mutable, value);
    }

    fn update_mutable<T, F: FnOnce(bevy::prelude::Mut<T>)>(&mut self, mutable: Entity, updater: F)
    where
        T: Send + Sync + 'static,
    {
        self.world.update_mutable(mutable, updater);
    }
}
