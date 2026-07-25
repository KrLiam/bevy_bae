#![doc = include_str!("../readme.md")]

/// Everything you need to get started with `bevy_bae`
pub mod prelude {
    pub use crate::{
        BaePlugin, BaeSystems, Estr,
        condition::{
            Condition,
            relationship::{
                ConditionOf, ConditionSpawner, ConditionSpawnerCommands, Conditions, conditions,
            },
        },
        effect::{
            Effect,
            relationship::{EffectOf, EffectSpawner, EffectSpawnerCommands, Effects, effects},
        },
        plan::{LogPlan, Plan, update::UpdatePlan},
        props::*,
        task::{
            compound::{
                CompoundTask,
                relationship::{TaskOf, TaskSpawner, TaskSpawnerCommands, Tasks, tasks},
                select::Select,
                sequence::Sequence,
            },
            operator::{Operator, OperatorInput, OperatorStatus},
        },
    };
    pub(crate) use {
        crate::name_ext::NameOrEntityExt as _,
        bevy_app::prelude::*,
        bevy_derive::{Deref, DerefMut},
        bevy_ecs::prelude::*,
        bevy_reflect::prelude::*,
        tracing::{self, debug, info},
    };
}
extern crate alloc;
use bevy_ecs::{intern::Interned, schedule::ScheduleLabel};
pub use estr::Estr;

use crate::{
    plan::{
        execution::{check_plan_on_prop_change, clear_operator_observers_on_inserted_plan, execute_plan, run_exit_operators_on_inserted_plan, run_exit_operators_on_removed_plan, update_empty_plans, update_unlocked_plan}, log_plan, update::update_plan,
    }, prelude::*, task::{
        compound::{CompoundAppExt, loop_task::Loop}, observer::register_observer_operator_systems, validation::{insert_bae_task_present_on_add, remove_bae_task_present_on_remove},
    },
};

pub mod condition;
pub mod effect;
mod name_ext;
pub mod plan;
pub mod props;
pub mod task;

/// The plugin required to use `bevy_bae`. The schedule used can be configured with [`Self::new`], and the default is [`FixedUpdate`].
pub struct BaePlugin {
    schedule: Interned<dyn ScheduleLabel>,
}

impl BaePlugin {
    /// Create a new plugin in the given schedule. The default is [`FixedUpdate`].
    pub fn new(schedule: impl ScheduleLabel) -> Self {
        Self {
            schedule: schedule.intern(),
        }
    }
}

impl Default for BaePlugin {
    fn default() -> Self {
        Self {
            schedule: FixedUpdate.intern(),
        }
    }
}
impl Plugin for BaePlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(self.schedule, (BaeSystems::ExecutePlan,).chain());
        app.world_mut().register_component::<Condition>();
        app.world_mut().register_component::<Effect>();
        app.add_observer(insert_bae_task_present_on_add::<Operator>)
            .add_observer(remove_bae_task_present_on_remove::<Operator>)
            .add_observer(insert_bae_task_present_on_add::<Tasks>)
            .add_observer(remove_bae_task_present_on_remove::<Tasks>);
        app.add_compound_task::<Select>()
            .add_compound_task::<Sequence>()
            .add_compound_task::<Loop>();
        app.add_observer(update_plan).add_observer(log_plan);
        app
            .add_systems(
                self.schedule,
                (
                (
                    check_plan_on_prop_change,
                    update_unlocked_plan,
                    update_empty_plans,
                    execute_plan
                )
                    .chain()
                    .in_set(BaeSystems::ExecutePlan),),
            )
            .add_observer(run_exit_operators_on_inserted_plan)
            .add_observer(run_exit_operators_on_removed_plan)
            .add_observer(clear_operator_observers_on_inserted_plan);
    
        register_observer_operator_systems(app.world_mut());
    }
}

/// System set used by all systems of `bevy_bae`.
#[derive(SystemSet, Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum BaeSystems {
    /// Executes [`Plan`]s, and replans them if necessary.
    ExecutePlan,
}
