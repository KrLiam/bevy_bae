//! Tests the plan execution

use bevy::{log::LogPlugin, prelude::*, time::TimeUpdateStrategy};
use bevy_bae::{plan::{PlanDomain, PlanReactivity}, prelude::*, task::{compound::loop_task::Loop, scope::{EnterOperator, ExitOperator}}};
use bevy_ecs::entity_disabling::Disabled;
use std::sync::Mutex;

#[test]
fn runs_plan() {
    let mut app = App::test(op("a"));
    app.update();
    app.assert_last_opt("a");
}

#[test]
fn runs_plan_with_condition() {
    let mut app = App::test((Select, tasks![(op("a"), cond_is("use_a", true)), op("b")]));
    app.update();
    app.assert_last_opt("b");
}

#[test]
fn skips_plan_only_effect() {
    let mut app = App::test((
        Sequence,
        tasks![
            (op("a"), effects![Effect::set("use_b", true).plan_only()]),
            (op("b"), cond_is("use_b", true)),
        ],
    ));
    // plan
    app.update();
    app.assert_last_opt("a");

    // try to run b, but we didn't actually set use_b, so abort plan
    app.update();
    app.assert_last_opt(None);

    // replan same plan
    app.update();
    app.assert_last_opt("a");
}

#[test]
fn runs_plan_then_replans_with_new_effects() {
    let mut app = App::test((
        Select,
        tasks![
            (op("a"), cond_is("use_b", false), eff("use_b", true)),
            op("b")
        ],
    ));
    // plan
    app.update();
    app.assert_last_opt("a");
    // replan
    app.update();
    app.assert_last_opt("b");
    // replan to same plan
    app.update();
    app.assert_last_opt("b");
}

#[test]
fn replans_on_invalid_conditions() {
    let mut app = App::test((
        Select,
        tasks![
            (
                Sequence,
                tasks![op("a"), (op("b"), cond_is("disabled", false)),]
            ),
            op("c"),
        ],
    ));
    // plan
    app.update();
    app.assert_last_opt("a");
    app.behavior_entity().props_mut().set("disabled", true);
    // abort plan
    app.update();
    app.assert_last_opt(None);
    // replan
    app.update();
    app.assert_last_opt("c");
}

#[test]
fn ignores_disabled_behavior() {
    let mut app = App::test((
        Select,
        tasks![
            (op("a"), cond_is("use_b", false), eff("use_b", true)),
            op("b")
        ],
    ));
    // plan
    app.update();
    app.assert_last_opt("a");

    // disable behavior
    app.behavior_entity().insert(Disabled);
    app.update();
    app.assert_last_opt(None);

    // enable behavior and replan
    app.behavior_entity().remove::<Disabled>();
    app.update();
    app.assert_last_opt("b");
}

#[test]
fn replan_keeps_self() {
    let mut app = App::test((Sequence, tasks![op("a"), op("b")]));
    app.update();
    app.assert_last_opt("a");

    app.behavior_entity().trigger(UpdatePlan::new);

    app.update();
    app.assert_last_opt("b");
}

#[test]
fn replan_keeps_higher_priority() {
    let mut app = App::test((
        Select,
        tasks![
            (
                Sequence,
                tasks![
                    (op("a"), cond_is("disabled", false), eff("disabled", true)),
                    op("b")
                ]
            ),
            (Sequence, tasks![op("c"), op("d")])
        ],
    ));
    app.update();
    app.assert_last_opt("a");

    app.behavior_entity().trigger(UpdatePlan::new);

    // Even though our current sequence is no longer valid, we need to finish it before switching away due to a replan.
    app.update();
    app.assert_last_opt("b");

    app.update();
    app.assert_last_opt("c");

    app.update();
    app.assert_last_opt("d");
}

#[test]
fn replan_switches_to_higher_priority() {
    let mut app = App::test((
        Select,
        tasks![
            (
                Sequence,
                tasks![(op("a"), cond_is("enabled", true)), op("b")]
            ),
            (Sequence, tasks![(op("c"), eff("enabled", true)), op("d")])
        ],
    ));
    app.update();
    app.assert_last_opt("c");

    app.behavior_entity().trigger(UpdatePlan::new);

    app.update();
    app.assert_last_opt("a");

    app.update();
    app.assert_last_opt("b");

    app.update();
    app.assert_last_opt("a");
}

#[test]
fn does_not_replan_on_internal_prop_change() {
    let mut app = App::test((
        Select,
        tasks![
            (
                Sequence,
                tasks![(op("a"), cond_is("enabled", true)), op("b")]
            ),
            (Sequence, tasks![(op("c"), eff("enabled", true)), op("d")])
        ],
    ));
    app.update();
    app.assert_last_opt("c");

    app.update();
    app.assert_last_opt("d");

    app.update();
    app.assert_last_opt("a");

    app.update();
    app.assert_last_opt("b");

    app.update();
    app.assert_last_opt("a");
}

#[test]
fn does_not_replan_on_external_prop_change() {
    let mut app = App::test((
        Select,
        tasks![
            (
                Sequence,
                tasks![(op("a"), cond_is("enabled", true)), op("b")]
            ),
            (Sequence, tasks![op("c"), op("d")])
        ],
    ));
    app.update();
    app.assert_last_opt("c");

    app.behavior_entity().set_prop("enabled", true);

    app.update();
    app.assert_last_opt("d");

    app.update();
    app.assert_last_opt("a");

    app.update();
    app.assert_last_opt("b");

    app.update();
    app.assert_last_opt("a");
}

#[test]
fn higher_order_conditions_are_ignored_when_already_in_task() {
    let mut app = App::test((
        Select,
        tasks![
            (
                Sequence,
                cond_is("disabled", false),
                tasks![op("a"), op("b")]
            ),
            op("c")
        ],
    ));
    app.update();
    app.assert_last_opt("a");

    app.behavior_entity().set_prop("disabled", true);

    app.update();
    app.assert_last_opt("b");
}

#[test]
fn higher_order_conditions_are_respected_when_entering_task() {
    let mut app = App::test((
        Select,
        tasks![
            (
                Sequence,
                cond_is("disabled", false),
                tasks![op("a"), op("b")]
            ),
            op("c")
        ],
    ));

    app.behavior_entity().set_prop("disabled", true);
    app.update();
    app.assert_last_opt(None);
    app.update();
    app.assert_last_opt("c");
}

#[test]
fn compound_effects_are_applied() {
    let mut app = App::test((
        Select,
        tasks![
            (Sequence, tasks![op("a"), op("b")], eff("called", true),),
            op("c")
        ],
    ));
    app.update();
    app.assert_last_opt("a");

    assert!(!app.behavior_entity().get_prop::<bool>("called"));

    app.update();
    app.assert_last_opt("b");

    assert!(app.behavior_entity().get_prop::<bool>("called"));

    app.update();
    app.assert_last_opt("a");
}

#[test]
fn nested_compound_effects_are_applied() {
    let mut app = App::test((
        Select,
        eff("called_outer", true),
        tasks![
            (
                Sequence,
                eff("called_inner", true),
                tasks![op("a"), op("b")]
            ),
            op("c")
        ],
    ));
    app.update();
    app.assert_last_opt("a");

    assert!(!app.behavior_entity().get_prop::<bool>("called_outer"));
    assert!(!app.behavior_entity().get_prop::<bool>("called_inner"));

    app.update();
    app.assert_last_opt("b");

    assert!(app.behavior_entity().get_prop::<bool>("called_outer"));
    assert!(app.behavior_entity().get_prop::<bool>("called_inner"));

    app.update();
    app.assert_last_opt("a");
}

#[test]
fn compound_effects_are_not_applied_on_abort() {
    let mut app = App::test((
        Select,
        tasks![
            (
                Sequence,
                eff("called", true),
                tasks![op("a"), (op("b"), cond_is("disabled", false))]
            ),
            op("c")
        ],
    ));
    app.update();
    app.assert_last_opt("a");

    assert!(!app.behavior_entity().get_prop::<bool>("called"));

    app.behavior_entity().set_prop("disabled", true);
    app.update();
    app.assert_last_opt(None);
    assert!(!app.behavior_entity().get_prop::<bool>("called"));

    app.update();
    app.assert_last_opt("c");
    assert!(!app.behavior_entity().get_prop::<bool>("called"));
}

#[test]
fn nested_enter_exit_operator_on_success() {
    let mut app = App::test((
        Name::new("a"),
        Sequence,
        EnterOperator::new(scope_system("enter a")),
        tasks! [
            op("b"),
            (
                Name::new("c"),
                Operator::new(operator_system("c", 1)),
                EnterOperator::new(scope_system("enter c")),
                ExitOperator::new(scope_system("exit c")),
            )
        ],
        ExitOperator::new(scope_system("exit a")),
    ));

    app.update();
    assert_eq!(app.opt_log(), vec!["enter a", "b"]);

    app.update();
    assert_eq!(app.opt_log(), vec!["enter a", "b", "enter c", "c", "exit c", "exit a"]);
}

#[test]
fn nested_enter_exit_operator_on_replan() {
    let mut app = App::test((
        Name::new("a"),
        Sequence,
        tasks! [
            (
                Name::new("b"),
                EnterOperator::new(scope_system("enter b")),
                Operator::new(operator_system("b", 1)),
            )
        ],
        ExitOperator::new(scope_system("exit a")),
    ));

    app.update();
    assert_eq!(app.opt_log(), vec!["enter b", "b", "exit a"]);

    app.update();
    assert_eq!(app.opt_log(), vec!["enter b", "b", "exit a", "enter b", "b", "exit a"]);
}

#[test]
fn exit_operator_on_success_after_ongoing() {
    let mut app = App::test((
        Sequence,
        tasks! [
            (
                Name::new("a"),
                EnterOperator::new(scope_system("enter a")),
                Operator::new(operator_system("a", 3)),
                ExitOperator::new(scope_system("exit a")),
            ),
            op("b")
        ]
    ));

    app.update();
    assert_eq!(app.opt_log(), vec!["enter a", "a"]);

    app.update();
    assert_eq!(app.opt_log(), vec!["enter a", "a", "a"]);

    app.update();
    assert_eq!(app.opt_log(), vec!["enter a", "a", "a", "a", "exit a"]);

    app.update();
    assert_eq!(app.opt_log(), vec!["enter a", "a", "a", "a", "exit a", "b"]);    
}

#[test]
fn exit_operator_on_plan_update() {
    let mut app = App::test((
        Select,
        tasks! [
            (
                cond_is("run_a", true),
                op("a"),
            ),
            (
                Name::new("b"),
                Operator::new(operator_system("b", 3)),
                ExitOperator::new(scope_system("exit b")),
            ),
        ],
        ExitOperator::new(scope_system("exit root")),
    ));
    let root = app.get_entity("root");

    app.update();
    assert_eq!(app.opt_log(), vec!["b"]);

    app.get_props_mut("root").set("run_a", true);
    app.world_mut().commands().trigger(UpdatePlan::new(root));

    app.update();
    assert_eq!(app.opt_log(), vec!["b", "exit b", "exit root", "a", "exit root"]);
}

#[test]
fn exit_operator_on_new_plan_inserted() {
    let mut app = App::test((
        Sequence,
        tasks! [
            (
                Name::new("a"),
                Operator::new(operator_system("a", 4)),
                ExitOperator::new(scope_system("exit a")),
            ),
        ],
        ExitOperator::new(scope_system("exit root")),
    ));
    let root = app.get_entity("root");

    app.update();
    assert_eq!(app.opt_log(), vec!["a"]);

    app.update();
    assert_eq!(app.opt_log(), vec!["a", "a"]);

    app.world_mut().entity_mut(root).insert(Plan::default());

    app.update();
    assert_eq!(app.opt_log(), vec!["a", "a", "exit a", "exit root", "a"]);
}

#[test]
fn exit_operator_on_plan_removed() {
    let mut app = App::test((
        Sequence,
        tasks! [
            (
                Name::new("a"),
                Operator::new(operator_system("a", 3)),
                ExitOperator::new(scope_system("exit a")),
            ),
        ],
        ExitOperator::new(scope_system("exit root")),
    ));
    let root = app.get_entity("root");

    app.update();
    assert_eq!(app.opt_log(), vec!["a"]);

    app.update();
    assert_eq!(app.opt_log(), vec!["a", "a"]);

    app.world_mut().entity_mut(root).remove::<Plan>();

    app.update();
    assert_eq!(app.opt_log(), vec!["a", "a", "exit a", "exit root"]);
}

#[test]
fn select_with_reactivity() {
    let mut app = App::test((
        PlanReactivity::default(),
        Select,
        tasks! [
            (
                cond_is("flag", true),
                Sequence,
                tasks! [
                    op("a 1"),
                    op("a 2"),
                ]
            ),
            (
                Sequence,
                tasks! [
                    op("b 1"),
                    op("b 2"),
                    op("b 3"),
                ]
            ),
        ]
    ));

    app.update();
    app.assert_last_opt("b 1");

    let mut props = app.get_props_mut("root");
    props.set("unused_flag", true);
    
    app.update();
    app.assert_last_opt("b 2");
    
    let mut props = app.get_props_mut("root");
    props.set("flag", true);

    app.update();
    app.assert_last_opt("a 1");
}

#[test]
fn loop_with_enter_exit_operator() {
    let mut app = App::test((
        Name::new("a"),
        Loop,
        EnterOperator::new(scope_system("enter a")),
        tasks! [
            (
                Name::new("b"),
                EnterOperator::new(scope_system("enter b")),
                Operator::new(operator_system("b", 1)),
            ),
            op("c"),
            (
                Name::new("d"),
                Operator::new(operator_system("d", 1)),
                ExitOperator::new(scope_system("exit d")),
            ),
        ],
        ExitOperator::new(scope_system("exit a")),
    ));

    app.update();
    assert_eq!(app.opt_log(), vec!["enter a", "enter b", "b"]);

    app.update();
    assert_eq!(app.opt_log(), vec!["enter a", "enter b", "b", "c"]);

    app.update();
    assert_eq!(app.opt_log(), vec!["enter a", "enter b", "b", "c", "d", "exit d"]);

    app.update();
    assert_eq!(app.opt_log(), vec!["enter a", "enter b", "b", "c", "d", "exit d", "enter b", "b"]);

    app.update();
    assert_eq!(app.opt_log(), vec!["enter a", "enter b", "b", "c", "d", "exit d", "enter b", "b", "c"]);

    let root = app.get_entity("a");
    app.world_mut().entity_mut(root).insert(Plan::default());

    app.update();
    assert_eq!(app.opt_log(), vec![
        "enter a", "enter b", "b", "c", "d", "exit d", "enter b", "b", "c",
        "exit a", "enter a", "enter b", "b"
    ]);
}

#[test]
fn logs_plan() {
    let mut app = App::test((
        Select,
        tasks![
            (
                Sequence,
                eff("called", true),
                tasks![op("a"), (op("b"), cond_is("disabled", false))]
            ),
            op("c")
        ],
    ));
    app.update();
    app.assert_last_opt("a");

    app.behavior_entity().trigger(LogPlan::new);

    app.update();
}

trait TestApp {
    fn test(behavior: impl Bundle) -> App;
    #[track_caller]
    fn assert_last_opt(&self, name: impl Into<Option<&'static str>>);
    fn opt_log(&self) -> Vec<String>;
    fn behavior_entity(&mut self) -> EntityWorldMut<'_>;
    fn get_entity<'a>(&'a mut self, name: &'static str) -> Entity;
    fn get_props_mut<'a>(&'a mut self, name: &'static str) -> Mut<'a, Props>;
}

impl TestApp for App {
    fn test(behavior: impl Bundle) -> App {
        let mut app = App::new();
        let behavior = Mutex::new(Some(behavior));
        app.add_plugins((
            MinimalPlugins,
            LogPlugin {
                filter: format!(
                    "bevy_log=off,bevy_bae=debug,{default}",
                    default = bevy::log::DEFAULT_FILTER
                ),
                ..default()
            },
            BaePlugin::default(),
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            Time::<Fixed>::default().timestep(),
        ))
        .init_resource::<OptLog>()
        .add_systems(Startup, move |mut commands: Commands| {
            commands
                .spawn(behavior.lock().unwrap().take().unwrap())
                .insert_if_new(Name::new("root"))
                .insert_if_new(PlanDomain::default())
                .trigger(UpdatePlan::new);
        })
        .add_systems(PreUpdate, |mut last_opt: ResMut<OptLog>| {
            last_opt.1 = last_opt.0.len();
        });
        app.finish();
        app.update();
        app.assert_last_opt(None);
        app
    }

    #[track_caller]
    fn assert_last_opt(&self, expected: impl Into<Option<&'static str>>) {
        let expected: Option<&'static str> = expected.into();
        let expected: Option<String> = expected.map(Into::into);
        let log = self.world().resource::<OptLog>();
        let actual = if log.0.len() > log.1 { log.0.last().cloned() } else { None };
        assert_eq!(expected, actual);
    }

    #[track_caller]
    fn opt_log(&self) -> Vec<String> {
        let actual = self.world().resource::<OptLog>().0.clone();
        actual
    }

    fn behavior_entity(&mut self) -> EntityWorldMut<'_> {
        let entity = self
            .world()
            .try_query_filtered::<Entity, (With<Plan>, Allow<Disabled>)>()
            .unwrap()
            .single(self.world())
            .unwrap();
        self.world_mut().entity_mut(entity)
    }

    fn get_entity<'a>(&'a mut self, name: &'static str) -> Entity {
        let mut q = self.world_mut().query::<(Entity, &Name)>();
        let (entity, _) = q.iter_mut(self.world_mut())
            .find(|(_, entity_name)| entity_name.as_str() == name)
            .unwrap();
        entity
    }

    fn get_props_mut<'a>(&'a mut self, name: &'static str) -> Mut<'a, Props> {
        let entity = self.get_entity(name);
        self.world_mut().get_mut::<Props>(entity).unwrap()
    }
}
// The following functions are not reflective of real user code and are here to make the test suite more simple to set up.


#[derive(Resource, Default)]
struct OptLog(Vec<String>, usize);

fn operator_system(name: impl Into<String>, duration: i32) -> impl FnMut(In<OperatorInput>, ResMut<OptLog>, Local<i32>) -> OperatorStatus {
    let name = name.into();
    move |
        _: In<OperatorInput>,
        mut opt_log: ResMut<OptLog>,
        mut counter: Local<i32>,
    | {
        *counter = *counter +1;
        opt_log.0.push(name.to_string());
        if *counter <= duration - 1 {
            OperatorStatus::Ongoing
        }
        else {
            OperatorStatus::Success
        }
    }
}

fn scope_system(name: impl Into<String>) -> impl FnMut(In<OperatorInput>, ResMut<OptLog>) {
    let name = name.into();
    move |_: In<OperatorInput>, mut opt_log: ResMut<OptLog>| {
        opt_log.0.push(name.to_string());
    }
}

fn op(name: &str) -> impl Bundle {
    let name = name.to_string();
    (
        Name::new(name.clone()),
        Operator::new(operator_system(name, 1)),
    )
}

fn cond_is(name: &str, val: impl Into<Value>) -> impl Bundle {
    conditions![Condition::eq(name, val)]
}

fn eff(name: &str, val: impl Into<Value>) -> impl Bundle {
    effects![Effect::set(name, val)]
}
