//! Defines the operators triggered when entering/exiting task scopes.

use core::fmt::Debug;
use bevy_ecs::{component::Component, lifecycle::HookContext, system::{Commands, In, IntoSystem, SystemId}, world::DeferredWorld};
use bevy_reflect::Reflect;

use crate::{prelude::*, task::validation::BaeTaskPresent};


/// The exact type of [`SystemId`] valid for [`Operator`]s.
pub type ScopeOperatorId = SystemId<In<OperatorInput>, ()>;


/// Specifies the system that is called right before the main task logic.
/// If the task execution spans multiple iterations, the enter system is called
/// only once.
#[derive(Component, Reflect)]
#[reflect(Component)]
#[component(on_insert = Self::on_insert_hook, on_replace = Self::on_replace_hook)]
#[require(BaeTaskPresent)]
pub struct EnterOperator {
    #[reflect(ignore)]
    register_system: Option<Box<dyn FnOnce(&mut Commands) -> ScopeOperatorId + Send + Sync>>,
    #[reflect(ignore)]
    system_id: Option<ScopeOperatorId>,
}

impl Default for EnterOperator {
    fn default() -> Self {
        Self::noop()
    }
}

impl Clone for EnterOperator {
    fn clone(&self) -> Self {
        Self {
            register_system: None,
            system_id: self.system_id,
        }
    }
}

impl PartialEq for EnterOperator {
    fn eq(&self, other: &Self) -> bool {
        self.system_id == other.system_id
    }
}

impl Eq for EnterOperator {}

impl Debug for EnterOperator {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Operator")
            .field("system_id", &self.system_id)
            .finish()
    }
}

impl EnterOperator {
    /// Creates a new operator using the provided system. The system must take [`OperatorInput`] as input and return an [`OperatorStatus`].
    pub fn new<S, M>(system: S) -> Self
    where
        S: IntoSystem<In<OperatorInput>, (), M>,
        S::System: Send + Sync + 'static,
    {
        let system = IntoSystem::into_system(system);
        Self {
            system_id: None,
            register_system: Some(Box::new(move |commands| commands.register_system(system))),
        }
    }

    /// Shorthand for creating an operator that does nothing.
    pub fn noop() -> Self {
        Self::new(|_: In<OperatorInput>| {})
    }

    /// Returns the [`SystemId`] of the registered operator one-shot system.
    pub fn system_id(&self) -> Option<ScopeOperatorId> {
        self.system_id
    }

    fn on_insert_hook(mut world: DeferredWorld, context: HookContext) {
        let Some(register_system) = world
            .get_mut::<Self>(context.entity)
            .and_then(|mut task_system| task_system.register_system.take())
        else {
            return;
        };
        let system_id = register_system(&mut world.commands());
        world.get_mut::<Self>(context.entity).unwrap().system_id = Some(system_id);
    }

    fn on_replace_hook(mut world: DeferredWorld, context: HookContext) {
        let Some(system_id) = world
            .get::<Self>(context.entity)
            .and_then(|tt| tt.system_id)
        else {
            return;
        };
        world.commands().unregister_system(system_id);
    }
}



/// Specifies the system that is called after the main task logic.
/// This system is guaranteed to run even if the plan is replaced.
#[derive(Component, Reflect)]
#[reflect(Component)]
#[component(on_insert = Self::on_insert_hook, on_replace = Self::on_replace_hook)]
#[require(BaeTaskPresent, EnterOperator)]
pub struct ExitOperator {
    #[reflect(ignore)]
    register_system: Option<Box<dyn FnOnce(&mut Commands) -> ScopeOperatorId + Send + Sync>>,
    #[reflect(ignore)]
    system_id: Option<ScopeOperatorId>,
}

impl Default for ExitOperator {
    fn default() -> Self {
        Self::noop()
    }
}

impl Clone for ExitOperator {
    fn clone(&self) -> Self {
        Self {
            register_system: None,
            system_id: self.system_id,
        }
    }
}

impl PartialEq for ExitOperator {
    fn eq(&self, other: &Self) -> bool {
        self.system_id == other.system_id
    }
}

impl Eq for ExitOperator {}

impl Debug for ExitOperator {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Operator")
            .field("system_id", &self.system_id)
            .finish()
    }
}

impl ExitOperator {
    /// Creates a new operator using the provided system. The system must take [`OperatorInput`] as input and return an [`OperatorStatus`].
    pub fn new<S, M>(system: S) -> Self
    where
        S: IntoSystem<In<OperatorInput>, (), M>,
        S::System: Send + Sync + 'static,
    {
        let system = IntoSystem::into_system(system);
        Self {
            system_id: None,
            register_system: Some(Box::new(move |commands| commands.register_system(system))),
        }
    }

    /// Shorthand for creating an operator that does nothing.
    pub fn noop() -> Self {
        Self { register_system: None, system_id: None }
    }

    /// Returns the [`SystemId`] of the registered operator one-shot system.
    pub fn system_id(&self) -> Option<ScopeOperatorId> {
        self.system_id
    }

    fn on_insert_hook(mut world: DeferredWorld, context: HookContext) {
        let Some(register_system) = world
            .get_mut::<Self>(context.entity)
            .and_then(|mut task_system| task_system.register_system.take())
        else {
            return;
        };
        let system_id = register_system(&mut world.commands());
        world.get_mut::<Self>(context.entity).unwrap().system_id = Some(system_id);
    }

    fn on_replace_hook(mut world: DeferredWorld, context: HookContext) {
        let Some(system_id) = world
            .get::<Self>(context.entity)
            .and_then(|tt| tt.system_id)
        else {
            return;
        };
        world.commands().unregister_system(system_id);
    }
}
