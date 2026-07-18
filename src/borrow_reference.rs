//! Independent generated reference model for a deliberately small ownership
//! subset. It does not inspect the Forgentex AST or call the production borrow
//! checker. Instead it evaluates availability of an owned pair and its two
//! move paths, then renders equivalent source programs for differential checks.

use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceMoveCase {
    pub id: String,
    pub description: String,
    pub source: String,
    pub expects_use_after_move: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceAliasCase {
    pub id: String,
    pub description: String,
    pub source: String,
    pub expects_mutable_borrow_alias: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceLifetimeCase {
    pub id: String,
    pub description: String,
    pub source: String,
    pub expects_escaping_reference: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferencePropertyCase {
    pub id: String,
    pub description: String,
    pub source: String,
    pub expected_diagnostic: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceShrinkCandidate {
    pub description: String,
    pub source: String,
    pub expected_diagnostic: Option<&'static str>,
    pub complexity: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceCampaignCase {
    pub id: String,
    pub description: String,
    pub source: String,
    pub expected_diagnostic: Option<&'static str>,
    pub replay: String,
    pub complexity: usize,
    pub shrink_candidates: Vec<ReferenceShrinkCandidate>,
}

pub const DEFAULT_SAFETY_CAMPAIGN_SEED: u64 = 0x7361_6665_5f76_3201;
pub const DEFAULT_SAFETY_CAMPAIGN_CASES_PER_FAMILY: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Root,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Read,
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Action {
    access: Access,
    place: Place,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Scenario {
    Linear(Vec<Action>),
    Branch {
        left: Action,
        right: Action,
        suffix: Action,
    },
    Loop {
        body: Action,
        suffix: Action,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MoveState {
    left_available: bool,
    right_available: bool,
    use_after_move: bool,
}

impl MoveState {
    fn initial() -> Self {
        Self {
            left_available: true,
            right_available: true,
            use_after_move: false,
        }
    }

    fn apply(mut self, action: Action) -> Self {
        let available = match action.place {
            Place::Root => self.left_available && self.right_available,
            Place::Left => self.left_available,
            Place::Right => self.right_available,
        };
        if !available {
            self.use_after_move = true;
            return self;
        }
        if action.access == Access::Move {
            match action.place {
                Place::Root => {
                    self.left_available = false;
                    self.right_available = false;
                }
                Place::Left => self.left_available = false,
                Place::Right => self.right_available = false,
            }
        }
        self
    }

    fn join(self, other: Self) -> Self {
        Self {
            // A place is definitely available after a branch only when it is
            // available on every incoming path.
            left_available: self.left_available && other.left_available,
            right_available: self.right_available && other.right_available,
            use_after_move: self.use_after_move || other.use_after_move,
        }
    }
}

const ACTIONS: [Action; 6] = [
    Action {
        access: Access::Read,
        place: Place::Root,
    },
    Action {
        access: Access::Move,
        place: Place::Root,
    },
    Action {
        access: Access::Read,
        place: Place::Left,
    },
    Action {
        access: Access::Move,
        place: Place::Left,
    },
    Action {
        access: Access::Read,
        place: Place::Right,
    },
    Action {
        access: Access::Move,
        place: Place::Right,
    },
];

/// Generate an exhaustive small-state matrix:
///
/// - every one-, two-, and three-action straight-line program (258);
/// - every one-action-per-arm branch followed by one action (216);
/// - every one-action two-iteration loop followed by one action (36).
///
/// The resulting 510 programs cover whole-value and partial-field moves,
/// definite-availability joins, and loop-carried move state.
pub fn generated_move_cases() -> Vec<ReferenceMoveCase> {
    let mut cases = Vec::with_capacity(510);
    for len in 1..=3 {
        let combinations = ACTIONS.len().pow(len);
        for encoded in 0..combinations {
            let actions = decode_actions(encoded, len);
            cases.push(build_case(Scenario::Linear(actions)));
        }
    }
    for left in ACTIONS {
        for right in ACTIONS {
            for suffix in ACTIONS {
                cases.push(build_case(Scenario::Branch {
                    left,
                    right,
                    suffix,
                }));
            }
        }
    }
    for body in ACTIONS {
        for suffix in ACTIONS {
            cases.push(build_case(Scenario::Loop { body, suffix }));
        }
    }
    debug_assert_eq!(cases.len(), 510);
    cases
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AliasHandle {
    Root,
    Parent,
    Child,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AliasProjection {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectedHandle {
    First,
    Second,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BranchOrder {
    DirectThenAlias,
    AliasThenDirect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeepProjection {
    BranchLeft,
    BranchRight,
    OtherLeft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoopOrder {
    AliasThenDirect,
    DirectThenAlias,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IndexProjection {
    ConstantZero,
    ConstantOne,
    DynamicI,
    DynamicJ,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReferenceOrigin {
    Parameter,
    Unproven,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReturnedArguments {
    Left,
    Right,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LifetimeProjection {
    Field,
    Index,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9e37_79b9_7f4a_7c15
            } else {
                seed
            },
        }
    }

    fn next(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.state = value;
        value
    }

    fn index(&mut self, bound: usize) -> usize {
        (self.next() as usize) % bound
    }
}

const ALIAS_HANDLES: [AliasHandle; 3] =
    [AliasHandle::Root, AliasHandle::Parent, AliasHandle::Child];

/// Generate seven exhaustive bounded alias matrices:
///
/// - 39 one-, two-, and three-write programs over a mutable root, its local
///   alias, and a child reborrow;
/// - 56 one-, two-, and three-write programs over two aliases of independently
///   selected `left`/`right` field projections;
/// - 16 conditional alias-reassignment programs whose joined target set is
///   checked against a direct field write before or after the alias's last use.
/// - 126 programs over aliases of three two-level field paths;
/// - 8 fixed-target loop programs checking same-root and disjoint-root writes
///   in both statement orders.
/// - 16 pairs of constant and dynamic index projections, where only distinct
///   constants prove disjointness;
/// - 16 conditional alias-target joins inside loops, checked in both statement
///   orders against the loop-carried target set.
///
/// The independent model uses descendant order, projection equality, future
/// use, and set union. It does not inspect the Forgentex AST or reuse production
/// place/liveness code.
pub fn generated_alias_cases() -> Vec<ReferenceAliasCase> {
    let mut cases = Vec::with_capacity(277);
    generate_linear_reborrow_cases(&mut cases);
    generate_projection_cases(&mut cases);
    generate_branch_join_cases(&mut cases);
    generate_deep_projection_cases(&mut cases);
    generate_loop_cases(&mut cases);
    generate_index_projection_cases(&mut cases);
    generate_branch_in_loop_cases(&mut cases);
    debug_assert_eq!(cases.len(), 277);
    cases
}

/// Generate 132 bounded lifetime-provenance programs:
///
/// - 6 direct parameter/unproven returns through zero-to-two local aliases;
/// - 4 conditional returns joining every pair of parameter/unproven origins;
/// - 48 helper-call returns varying the helper's returned argument set, both
///   caller argument origins, helper-local aliasing, and caller-local aliasing.
/// - 4 exhaustive two-arm match returns;
/// - 8 loop/while body-versus-fallback return pairs;
/// - 36 one-to-three-layer wrapper chains over selective helper contracts.
/// - 14 field/index projection returns covering local aliases, conditional
///   joins, and selected-versus-unselected helper arguments;
/// - 12 self-recursive and mutually recursive return cycles.
///
/// The model is a two-point origin lattice. Local aliases preserve origin;
/// conditional returns union origins; and a helper result is proven only when
/// every argument position in its return contract receives a parameter origin.
pub fn generated_lifetime_cases() -> Vec<ReferenceLifetimeCase> {
    let mut cases = Vec::with_capacity(132);
    generate_direct_lifetime_cases(&mut cases);
    generate_conditional_lifetime_cases(&mut cases);
    generate_call_lifetime_cases(&mut cases);
    generate_match_lifetime_cases(&mut cases);
    generate_loop_lifetime_cases(&mut cases);
    generate_wrapper_lifetime_cases(&mut cases);
    generate_projection_lifetime_cases(&mut cases);
    generate_recursive_lifetime_cases(&mut cases);
    debug_assert_eq!(cases.len(), 132);
    cases
}

/// Generate a deterministic, replayable soundness-property matrix beyond the
/// exhaustive small programs: 64 long move sequences, 64 long descendant-loan
/// sequences, 64 long projected-alias sequences, and 64 depth-4-to-8 lifetime
/// wrapper chains. Case IDs pin the generator family and iteration.
pub fn generated_safety_property_cases() -> Vec<ReferencePropertyCase> {
    let mut cases = Vec::with_capacity(256);
    generate_move_property_cases(&mut cases);
    generate_alias_property_cases(&mut cases);
    generate_projection_property_cases(&mut cases);
    generate_lifetime_property_cases(&mut cases);
    debug_assert_eq!(cases.len(), 256);
    cases
}

/// Generate a caller-seeded differential campaign. Unlike the fixed release
/// property matrix, this interface is intended for repeated CI and local
/// exploration. Every case carries a versioned replay command and a set of
/// smaller, model-equivalent candidates that the conformance runner can use to
/// minimize a production-checker mismatch.
pub fn generated_safety_campaign(seed: u64, cases_per_family: usize) -> Vec<ReferenceCampaignCase> {
    let mut cases = Vec::with_capacity(cases_per_family.saturating_mul(4));
    generate_campaign_move_cases(&mut cases, seed, cases_per_family);
    generate_campaign_alias_cases(&mut cases, seed, cases_per_family);
    generate_campaign_projection_cases(&mut cases, seed, cases_per_family);
    generate_campaign_lifetime_cases(&mut cases, seed, cases_per_family);
    cases
}

fn generate_move_property_cases(cases: &mut Vec<ReferencePropertyCase>) {
    let mut rng = DeterministicRng::new(0x6d6f_7665_5f76_3101);
    for iteration in 0..64 {
        let len = 4 + rng.index(9);
        let actions = (0..len)
            .map(|_| ACTIONS[rng.index(ACTIONS.len())])
            .collect::<Vec<_>>();
        let rejected = evaluate(&Scenario::Linear(actions.clone())).use_after_move;
        cases.push(ReferencePropertyCase {
            id: format!("reference.properties.moves.{iteration:03}"),
            description: format!(
                "seeded long move sequence len={len} expects {}",
                expected_word(rejected)
            ),
            source: render_source(&Scenario::Linear(actions)),
            expected_diagnostic: rejected.then_some("use_after_move"),
        });
    }
}

fn generate_alias_property_cases(cases: &mut Vec<ReferencePropertyCase>) {
    let mut rng = DeterministicRng::new(0x616c_6961_735f_3101);
    for iteration in 0..64 {
        let len = 4 + rng.index(9);
        let actions = (0..len)
            .map(|_| ALIAS_HANDLES[rng.index(ALIAS_HANDLES.len())])
            .collect::<Vec<_>>();
        let rejected = alias_sequence_conflicts(&actions);
        cases.push(ReferencePropertyCase {
            id: format!("reference.properties.aliases.{iteration:03}"),
            description: format!(
                "seeded long descendant-loan sequence len={len} expects {}",
                expected_word(rejected)
            ),
            source: render_alias_source(&actions),
            expected_diagnostic: rejected.then_some("mutable_borrow_alias"),
        });
    }
}

fn generate_projection_property_cases(cases: &mut Vec<ReferencePropertyCase>) {
    let mut rng = DeterministicRng::new(0x7072_6f6a_5f76_3101);
    for iteration in 0..64 {
        let first = [AliasProjection::Left, AliasProjection::Right][rng.index(2)];
        let second = [AliasProjection::Left, AliasProjection::Right][rng.index(2)];
        let len = 4 + rng.index(9);
        let actions = (0..len)
            .map(|_| [ProjectedHandle::First, ProjectedHandle::Second][rng.index(2)])
            .collect::<Vec<_>>();
        let rejected = projection_sequence_conflicts(first, second, &actions);
        cases.push(ReferencePropertyCase {
            id: format!("reference.properties.projections.{iteration:03}"),
            description: format!(
                "seeded long projection sequence len={len} expects {}",
                expected_word(rejected)
            ),
            source: render_projection_source(first, second, &actions),
            expected_diagnostic: rejected.then_some("mutable_borrow_alias"),
        });
    }
}

fn generate_lifetime_property_cases(cases: &mut Vec<ReferencePropertyCase>) {
    let mut rng = DeterministicRng::new(0x6c69_6665_5f76_3101);
    for iteration in 0..64 {
        let returned = [
            ReturnedArguments::Left,
            ReturnedArguments::Right,
            ReturnedArguments::Both,
        ][rng.index(3)];
        let left = [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven][rng.index(2)];
        let right = [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven][rng.index(2)];
        let depth = 4 + rng.index(5);
        let rejected = match returned {
            ReturnedArguments::Left => left == ReferenceOrigin::Unproven,
            ReturnedArguments::Right => right == ReferenceOrigin::Unproven,
            ReturnedArguments::Both => {
                left == ReferenceOrigin::Unproven || right == ReferenceOrigin::Unproven
            }
        };
        cases.push(ReferencePropertyCase {
            id: format!("reference.properties.lifetimes.{iteration:03}"),
            description: format!(
                "seeded deep lifetime wrapper depth={depth} expects {}",
                expected_word(rejected)
            ),
            source: render_wrapper_lifetime_source(returned, left, right, depth),
            expected_diagnostic: rejected.then_some("escaping_reference"),
        });
    }
}

fn generate_campaign_move_cases(cases: &mut Vec<ReferenceCampaignCase>, seed: u64, count: usize) {
    let mut rng = DeterministicRng::new(mixed_campaign_seed(seed, 0x6d6f_7665));
    for iteration in 0..count {
        let len = 8 + rng.index(17);
        let actions = (0..len)
            .map(|_| ACTIONS[rng.index(ACTIONS.len())])
            .collect::<Vec<_>>();
        let rejected = evaluate(&Scenario::Linear(actions.clone())).use_after_move;
        let expected_diagnostic = rejected.then_some("use_after_move");
        let id = campaign_case_id(seed, "moves", iteration);
        cases.push(ReferenceCampaignCase {
            description: format!(
                "runtime-seeded move campaign len={len} expects {}",
                expected_word(rejected)
            ),
            source: render_source(&Scenario::Linear(actions.clone())),
            replay: campaign_replay(seed, count, &id),
            complexity: len,
            shrink_candidates: move_shrink_candidates(&actions, expected_diagnostic),
            id,
            expected_diagnostic,
        });
    }
}

fn generate_campaign_alias_cases(cases: &mut Vec<ReferenceCampaignCase>, seed: u64, count: usize) {
    let mut rng = DeterministicRng::new(mixed_campaign_seed(seed, 0x616c_6961));
    for iteration in 0..count {
        let len = 8 + rng.index(17);
        let actions = (0..len)
            .map(|_| ALIAS_HANDLES[rng.index(ALIAS_HANDLES.len())])
            .collect::<Vec<_>>();
        let rejected = alias_sequence_conflicts(&actions);
        let expected_diagnostic = rejected.then_some("mutable_borrow_alias");
        let id = campaign_case_id(seed, "aliases", iteration);
        cases.push(ReferenceCampaignCase {
            description: format!(
                "runtime-seeded descendant-loan campaign len={len} expects {}",
                expected_word(rejected)
            ),
            source: render_alias_source(&actions),
            replay: campaign_replay(seed, count, &id),
            complexity: len,
            shrink_candidates: alias_shrink_candidates(&actions, expected_diagnostic),
            id,
            expected_diagnostic,
        });
    }
}

fn generate_campaign_projection_cases(
    cases: &mut Vec<ReferenceCampaignCase>,
    seed: u64,
    count: usize,
) {
    let mut rng = DeterministicRng::new(mixed_campaign_seed(seed, 0x7072_6f6a));
    for iteration in 0..count {
        let first = [AliasProjection::Left, AliasProjection::Right][rng.index(2)];
        let second = [AliasProjection::Left, AliasProjection::Right][rng.index(2)];
        let len = 8 + rng.index(17);
        let actions = (0..len)
            .map(|_| [ProjectedHandle::First, ProjectedHandle::Second][rng.index(2)])
            .collect::<Vec<_>>();
        let rejected = projection_sequence_conflicts(first, second, &actions);
        let expected_diagnostic = rejected.then_some("mutable_borrow_alias");
        let id = campaign_case_id(seed, "projections", iteration);
        cases.push(ReferenceCampaignCase {
            description: format!(
                "runtime-seeded projection campaign len={len} expects {}",
                expected_word(rejected)
            ),
            source: render_projection_source(first, second, &actions),
            replay: campaign_replay(seed, count, &id),
            complexity: len,
            shrink_candidates: projection_shrink_candidates(
                first,
                second,
                &actions,
                expected_diagnostic,
            ),
            id,
            expected_diagnostic,
        });
    }
}

fn generate_campaign_lifetime_cases(
    cases: &mut Vec<ReferenceCampaignCase>,
    seed: u64,
    count: usize,
) {
    let mut rng = DeterministicRng::new(mixed_campaign_seed(seed, 0x6c69_6665));
    for iteration in 0..count {
        let returned = [
            ReturnedArguments::Left,
            ReturnedArguments::Right,
            ReturnedArguments::Both,
        ][rng.index(3)];
        let left = [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven][rng.index(2)];
        let right = [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven][rng.index(2)];
        let depth = 8 + rng.index(9);
        let rejected = lifetime_wrapper_rejected(returned, left, right);
        let expected_diagnostic = rejected.then_some("escaping_reference");
        let id = campaign_case_id(seed, "lifetimes", iteration);
        cases.push(ReferenceCampaignCase {
            description: format!(
                "runtime-seeded lifetime campaign depth={depth} expects {}",
                expected_word(rejected)
            ),
            source: render_wrapper_lifetime_source(returned, left, right, depth),
            replay: campaign_replay(seed, count, &id),
            complexity: depth,
            shrink_candidates: (0..depth)
                .map(|candidate_depth| ReferenceShrinkCandidate {
                    description: format!("lifetime wrapper depth={candidate_depth}"),
                    source: render_wrapper_lifetime_source(returned, left, right, candidate_depth),
                    expected_diagnostic,
                    complexity: candidate_depth,
                })
                .collect(),
            id,
            expected_diagnostic,
        });
    }
}

fn lifetime_wrapper_rejected(
    returned: ReturnedArguments,
    left: ReferenceOrigin,
    right: ReferenceOrigin,
) -> bool {
    match returned {
        ReturnedArguments::Left => left == ReferenceOrigin::Unproven,
        ReturnedArguments::Right => right == ReferenceOrigin::Unproven,
        ReturnedArguments::Both => {
            left == ReferenceOrigin::Unproven || right == ReferenceOrigin::Unproven
        }
    }
}

fn move_shrink_candidates(
    actions: &[Action],
    expected_diagnostic: Option<&'static str>,
) -> Vec<ReferenceShrinkCandidate> {
    contiguous_shrinks(actions, expected_diagnostic, |candidate| {
        let rejected = evaluate(&Scenario::Linear(candidate.to_vec())).use_after_move;
        (rejected.then_some("use_after_move") == expected_diagnostic)
            .then(|| render_source(&Scenario::Linear(candidate.to_vec())))
    })
}

fn alias_shrink_candidates(
    actions: &[AliasHandle],
    expected_diagnostic: Option<&'static str>,
) -> Vec<ReferenceShrinkCandidate> {
    contiguous_shrinks(actions, expected_diagnostic, |candidate| {
        (alias_sequence_conflicts(candidate).then_some("mutable_borrow_alias")
            == expected_diagnostic)
            .then(|| render_alias_source(candidate))
    })
}

fn projection_shrink_candidates(
    first: AliasProjection,
    second: AliasProjection,
    actions: &[ProjectedHandle],
    expected_diagnostic: Option<&'static str>,
) -> Vec<ReferenceShrinkCandidate> {
    contiguous_shrinks(actions, expected_diagnostic, |candidate| {
        (projection_sequence_conflicts(first, second, candidate).then_some("mutable_borrow_alias")
            == expected_diagnostic)
            .then(|| render_projection_source(first, second, candidate))
    })
}

fn contiguous_shrinks<T>(
    items: &[T],
    expected_diagnostic: Option<&'static str>,
    mut render_if_equivalent: impl FnMut(&[T]) -> Option<String>,
) -> Vec<ReferenceShrinkCandidate> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    for target_len in 0..items.len() {
        let starts = if target_len == 0 {
            0..1
        } else {
            0..items.len() - target_len + 1
        };
        for start in starts {
            let candidate = &items[start..start + target_len];
            let Some(source) = render_if_equivalent(candidate) else {
                continue;
            };
            if seen.insert(source.clone()) {
                candidates.push(ReferenceShrinkCandidate {
                    description: format!("contiguous model-equivalent length={target_len}"),
                    source,
                    expected_diagnostic,
                    complexity: target_len,
                });
            }
        }
    }
    candidates
}

fn mixed_campaign_seed(seed: u64, domain: u64) -> u64 {
    let mut value = seed ^ domain.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn campaign_case_id(seed: u64, family: &str, iteration: usize) -> String {
    format!("reference.campaign.v1.{seed:016x}.{family}.{iteration:04}")
}

fn campaign_replay(seed: u64, count: usize, id: &str) -> String {
    format!(
        "fgx borrow-conformance --campaign-seed 0x{seed:016x} --campaign-cases {count} --json # {id}"
    )
}

fn generate_direct_lifetime_cases(cases: &mut Vec<ReferenceLifetimeCase>) {
    for origin in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
        for alias_depth in 0..=2 {
            let expects_escaping_reference = origin == ReferenceOrigin::Unproven;
            let scenario = format!("{}_aliases_{alias_depth}", origin_id(origin));
            cases.push(ReferenceLifetimeCase {
                id: format!("reference.lifetimes.direct.{scenario}"),
                description: format!(
                    "independent direct-origin model expects {} for {scenario}",
                    expected_word(expects_escaping_reference)
                ),
                source: render_direct_lifetime_source(origin, alias_depth),
                expects_escaping_reference,
            });
        }
    }
}

fn generate_conditional_lifetime_cases(cases: &mut Vec<ReferenceLifetimeCase>) {
    for left in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
        for right in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
            let expects_escaping_reference =
                left == ReferenceOrigin::Unproven || right == ReferenceOrigin::Unproven;
            let scenario = format!("{}_{}", origin_id(left), origin_id(right));
            cases.push(ReferenceLifetimeCase {
                id: format!("reference.lifetimes.conditional.{scenario}"),
                description: format!(
                    "independent conditional-origin join expects {} for {scenario}",
                    expected_word(expects_escaping_reference)
                ),
                source: render_conditional_lifetime_source(left, right),
                expects_escaping_reference,
            });
        }
    }
}

fn generate_call_lifetime_cases(cases: &mut Vec<ReferenceLifetimeCase>) {
    for returned in [
        ReturnedArguments::Left,
        ReturnedArguments::Right,
        ReturnedArguments::Both,
    ] {
        for left in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
            for right in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
                for helper_alias in [false, true] {
                    for caller_alias in [false, true] {
                        let returned_origins = match returned {
                            ReturnedArguments::Left => [Some(left), None],
                            ReturnedArguments::Right => [None, Some(right)],
                            ReturnedArguments::Both => [Some(left), Some(right)],
                        };
                        let expects_escaping_reference = returned_origins
                            .into_iter()
                            .flatten()
                            .any(|origin| origin == ReferenceOrigin::Unproven);
                        let scenario = format!(
                            "{}_{}_{}_helper_alias_{}_caller_alias_{}",
                            returned_arguments_id(returned),
                            origin_id(left),
                            origin_id(right),
                            helper_alias,
                            caller_alias
                        );
                        cases.push(ReferenceLifetimeCase {
                            id: format!("reference.lifetimes.call.{scenario}"),
                            description: format!(
                                "independent returned-argument contract expects {} for {scenario}",
                                expected_word(expects_escaping_reference)
                            ),
                            source: render_call_lifetime_source(
                                returned,
                                left,
                                right,
                                helper_alias,
                                caller_alias,
                            ),
                            expects_escaping_reference,
                        });
                    }
                }
            }
        }
    }
}

fn generate_match_lifetime_cases(cases: &mut Vec<ReferenceLifetimeCase>) {
    for left in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
        for right in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
            let expects_escaping_reference =
                left == ReferenceOrigin::Unproven || right == ReferenceOrigin::Unproven;
            let scenario = format!("{}_{}", origin_id(left), origin_id(right));
            cases.push(ReferenceLifetimeCase {
                id: format!("reference.lifetimes.match.{scenario}"),
                description: format!(
                    "independent match-arm origin join expects {} for {scenario}",
                    expected_word(expects_escaping_reference)
                ),
                source: render_match_lifetime_source(left, right),
                expects_escaping_reference,
            });
        }
    }
}

fn generate_loop_lifetime_cases(cases: &mut Vec<ReferenceLifetimeCase>) {
    for kind in ["for", "while"] {
        for body in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
            for fallback in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
                let expects_escaping_reference =
                    body == ReferenceOrigin::Unproven || fallback == ReferenceOrigin::Unproven;
                let scenario = format!("{}_{}_{}", kind, origin_id(body), origin_id(fallback));
                cases.push(ReferenceLifetimeCase {
                    id: format!("reference.lifetimes.loop.{scenario}"),
                    description: format!(
                        "independent loop/fallback origin join expects {} for {scenario}",
                        expected_word(expects_escaping_reference)
                    ),
                    source: render_loop_lifetime_source(kind, body, fallback),
                    expects_escaping_reference,
                });
            }
        }
    }
}

fn generate_wrapper_lifetime_cases(cases: &mut Vec<ReferenceLifetimeCase>) {
    for returned in [
        ReturnedArguments::Left,
        ReturnedArguments::Right,
        ReturnedArguments::Both,
    ] {
        for left in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
            for right in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
                for depth in 1..=3 {
                    let expects_escaping_reference = match returned {
                        ReturnedArguments::Left => left == ReferenceOrigin::Unproven,
                        ReturnedArguments::Right => right == ReferenceOrigin::Unproven,
                        ReturnedArguments::Both => {
                            left == ReferenceOrigin::Unproven || right == ReferenceOrigin::Unproven
                        }
                    };
                    let scenario = format!(
                        "{}_{}_{}_depth_{}",
                        returned_arguments_id(returned),
                        origin_id(left),
                        origin_id(right),
                        depth
                    );
                    cases.push(ReferenceLifetimeCase {
                        id: format!("reference.lifetimes.wrapper.{scenario}"),
                        description: format!(
                            "independent transitive return contract expects {} for {scenario}",
                            expected_word(expects_escaping_reference)
                        ),
                        source: render_wrapper_lifetime_source(returned, left, right, depth),
                        expects_escaping_reference,
                    });
                }
            }
        }
    }
}

fn generate_projection_lifetime_cases(cases: &mut Vec<ReferenceLifetimeCase>) {
    for projection in [LifetimeProjection::Field, LifetimeProjection::Index] {
        for alias_depth in 0..=2 {
            let scenario = format!(
                "{}_aliases_{alias_depth}",
                lifetime_projection_id(projection)
            );
            cases.push(ReferenceLifetimeCase {
                id: format!("reference.lifetimes.projection.direct.{scenario}"),
                description: format!(
                    "independent projection-origin model expects rejection for {scenario}"
                ),
                source: render_direct_projection_lifetime_source(projection, alias_depth),
                expects_escaping_reference: true,
            });
        }
        for projection_first in [false, true] {
            let order = if projection_first {
                "projection_first"
            } else {
                "parameter_first"
            };
            let scenario = format!("{}_{}", lifetime_projection_id(projection), order);
            cases.push(ReferenceLifetimeCase {
                id: format!("reference.lifetimes.projection.conditional.{scenario}"),
                description: format!(
                    "independent projection/parameter join expects rejection for {scenario}"
                ),
                source: render_conditional_projection_lifetime_source(projection, projection_first),
                expects_escaping_reference: true,
            });
        }
        for projection_selected in [false, true] {
            let selection = if projection_selected {
                "selected"
            } else {
                "unselected"
            };
            let scenario = format!("{}_{}", lifetime_projection_id(projection), selection);
            cases.push(ReferenceLifetimeCase {
                id: format!("reference.lifetimes.projection.call.{scenario}"),
                description: format!(
                    "independent selective return contract expects {} for {scenario}",
                    expected_word(projection_selected)
                ),
                source: render_call_projection_lifetime_source(projection, projection_selected),
                expects_escaping_reference: projection_selected,
            });
        }
    }
}

fn generate_recursive_lifetime_cases(cases: &mut Vec<ReferenceLifetimeCase>) {
    for base in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
        for recursive_arg in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
            for alias in [false, true] {
                let scenario = format!(
                    "base_{}_recursive_{}_alias_{}",
                    origin_id(base),
                    origin_id(recursive_arg),
                    alias
                );
                cases.push(ReferenceLifetimeCase {
                    id: format!("reference.lifetimes.recursive.self.{scenario}"),
                    description: format!(
                        "independent recursive-cycle model expects rejection for {scenario}"
                    ),
                    source: render_self_recursive_lifetime_source(base, recursive_arg, alias),
                    expects_escaping_reference: true,
                });
            }
        }
    }
    for origin in [ReferenceOrigin::Parameter, ReferenceOrigin::Unproven] {
        for alias in [false, true] {
            let scenario = format!("{}_alias_{}", origin_id(origin), alias);
            cases.push(ReferenceLifetimeCase {
                id: format!("reference.lifetimes.recursive.mutual.{scenario}"),
                description: format!(
                    "independent mutual-recursion model expects rejection for {scenario}"
                ),
                source: render_mutual_recursive_lifetime_source(origin, alias),
                expects_escaping_reference: true,
            });
        }
    }
}

fn generate_linear_reborrow_cases(cases: &mut Vec<ReferenceAliasCase>) {
    for len in 1..=3 {
        let combinations = ALIAS_HANDLES.len().pow(len);
        for encoded in 0..combinations {
            let actions = decode_alias_handles(encoded, len);
            let expects_mutable_borrow_alias = alias_sequence_conflicts(&actions);
            let action_ids = actions
                .iter()
                .map(|handle| alias_handle_id(*handle))
                .collect::<Vec<_>>()
                .join("_");
            cases.push(ReferenceAliasCase {
                id: format!("reference.aliases.linear.{action_ids}"),
                description: format!(
                    "independent reborrow-liveness model expects {} for {action_ids}",
                    if expects_mutable_borrow_alias {
                        "rejection"
                    } else {
                        "acceptance"
                    }
                ),
                source: render_alias_source(&actions),
                expects_mutable_borrow_alias,
            });
        }
    }
}

fn generate_projection_cases(cases: &mut Vec<ReferenceAliasCase>) {
    for first in [AliasProjection::Left, AliasProjection::Right] {
        for second in [AliasProjection::Left, AliasProjection::Right] {
            for len in 1..=3 {
                let combinations = 2usize.pow(len);
                for encoded in 0..combinations {
                    let actions = decode_projected_handles(encoded, len);
                    let expects_mutable_borrow_alias =
                        projection_sequence_conflicts(first, second, &actions);
                    let action_ids = actions
                        .iter()
                        .map(|handle| projected_handle_id(*handle))
                        .collect::<Vec<_>>()
                        .join("_");
                    let scenario = format!(
                        "{}_{}.{}",
                        projection_id(first),
                        projection_id(second),
                        action_ids
                    );
                    cases.push(ReferenceAliasCase {
                        id: format!("reference.aliases.projection.{scenario}"),
                        description: format!(
                            "independent projection-overlap model expects {} for {scenario}",
                            expected_word(expects_mutable_borrow_alias)
                        ),
                        source: render_projection_source(first, second, &actions),
                        expects_mutable_borrow_alias,
                    });
                }
            }
        }
    }
}

fn generate_branch_join_cases(cases: &mut Vec<ReferenceAliasCase>) {
    for initial in [AliasProjection::Left, AliasProjection::Right] {
        for reassigned in [AliasProjection::Left, AliasProjection::Right] {
            for direct in [AliasProjection::Left, AliasProjection::Right] {
                for order in [BranchOrder::DirectThenAlias, BranchOrder::AliasThenDirect] {
                    let expects_mutable_borrow_alias = order == BranchOrder::DirectThenAlias
                        && (direct == initial || direct == reassigned);
                    let scenario = format!(
                        "{}_to_{}.{}_{}",
                        projection_id(initial),
                        projection_id(reassigned),
                        projection_id(direct),
                        branch_order_id(order)
                    );
                    cases.push(ReferenceAliasCase {
                        id: format!("reference.aliases.branch.{scenario}"),
                        description: format!(
                            "independent conditional-place-set model expects {} for {scenario}",
                            expected_word(expects_mutable_borrow_alias)
                        ),
                        source: render_branch_source(initial, reassigned, direct, order),
                        expects_mutable_borrow_alias,
                    });
                }
            }
        }
    }
}

fn generate_deep_projection_cases(cases: &mut Vec<ReferenceAliasCase>) {
    let projections = [
        DeepProjection::BranchLeft,
        DeepProjection::BranchRight,
        DeepProjection::OtherLeft,
    ];
    for first in projections {
        for second in projections {
            for len in 1..=3 {
                let combinations = 2usize.pow(len);
                for encoded in 0..combinations {
                    let actions = decode_projected_handles(encoded, len);
                    let expects_mutable_borrow_alias =
                        first == second && projected_sequence_uses_both_live(&actions);
                    let action_ids = actions
                        .iter()
                        .map(|handle| projected_handle_id(*handle))
                        .collect::<Vec<_>>()
                        .join("_");
                    let scenario = format!(
                        "{}_{}.{}",
                        deep_projection_id(first),
                        deep_projection_id(second),
                        action_ids
                    );
                    cases.push(ReferenceAliasCase {
                        id: format!("reference.aliases.deep_projection.{scenario}"),
                        description: format!(
                            "independent nested-projection model expects {} for {scenario}",
                            expected_word(expects_mutable_borrow_alias)
                        ),
                        source: render_deep_projection_source(first, second, &actions),
                        expects_mutable_borrow_alias,
                    });
                }
            }
        }
    }
}

fn generate_loop_cases(cases: &mut Vec<ReferenceAliasCase>) {
    for alias in [AliasProjection::Left, AliasProjection::Right] {
        for direct in [AliasProjection::Left, AliasProjection::Right] {
            for order in [LoopOrder::AliasThenDirect, LoopOrder::DirectThenAlias] {
                // Independently unrolling the loop twice makes the alias live
                // after the direct write in either order when both target the
                // same root. Different roots never overlap.
                let expects_mutable_borrow_alias = alias == direct;
                let scenario = format!(
                    "{}_{}.{}",
                    projection_id(alias),
                    projection_id(direct),
                    loop_order_id(order)
                );
                cases.push(ReferenceAliasCase {
                    id: format!("reference.aliases.loop.{scenario}"),
                    description: format!(
                        "independent two-iteration alias model expects {} for {scenario}",
                        expected_word(expects_mutable_borrow_alias)
                    ),
                    source: render_loop_source(alias, direct, order),
                    expects_mutable_borrow_alias,
                });
            }
        }
    }
}

fn generate_index_projection_cases(cases: &mut Vec<ReferenceAliasCase>) {
    let projections = [
        IndexProjection::ConstantZero,
        IndexProjection::ConstantOne,
        IndexProjection::DynamicI,
        IndexProjection::DynamicJ,
    ];
    for first in projections {
        for second in projections {
            let expects_mutable_borrow_alias = index_projections_overlap(first, second);
            let scenario = format!(
                "{}_{}",
                index_projection_id(first),
                index_projection_id(second)
            );
            cases.push(ReferenceAliasCase {
                id: format!("reference.aliases.index.{scenario}"),
                description: format!(
                    "independent index-place model expects {} for {scenario}",
                    expected_word(expects_mutable_borrow_alias)
                ),
                source: render_index_projection_source(first, second),
                expects_mutable_borrow_alias,
            });
        }
    }
}

fn generate_branch_in_loop_cases(cases: &mut Vec<ReferenceAliasCase>) {
    for initial in [AliasProjection::Left, AliasProjection::Right] {
        for reassigned in [AliasProjection::Left, AliasProjection::Right] {
            for direct in [AliasProjection::Left, AliasProjection::Right] {
                for order in [LoopOrder::AliasThenDirect, LoopOrder::DirectThenAlias] {
                    let expects_mutable_borrow_alias = direct == initial || direct == reassigned;
                    let scenario = format!(
                        "{}_to_{}.{}_{}",
                        projection_id(initial),
                        projection_id(reassigned),
                        projection_id(direct),
                        loop_order_id(order)
                    );
                    cases.push(ReferenceAliasCase {
                        id: format!("reference.aliases.branch_in_loop.{scenario}"),
                        description: format!(
                            "independent loop-carried conditional target-set model expects {} for {scenario}",
                            expected_word(expects_mutable_borrow_alias)
                        ),
                        source: render_branch_in_loop_source(initial, reassigned, direct, order),
                        expects_mutable_borrow_alias,
                    });
                }
            }
        }
    }
}

fn index_projections_overlap(first: IndexProjection, second: IndexProjection) -> bool {
    !matches!(
        (first, second),
        (IndexProjection::ConstantZero, IndexProjection::ConstantOne)
            | (IndexProjection::ConstantOne, IndexProjection::ConstantZero)
    )
}

fn decode_alias_handles(mut encoded: usize, len: u32) -> Vec<AliasHandle> {
    let mut actions = Vec::with_capacity(len as usize);
    for _ in 0..len {
        actions.push(ALIAS_HANDLES[encoded % ALIAS_HANDLES.len()]);
        encoded /= ALIAS_HANDLES.len();
    }
    actions
}

fn alias_sequence_conflicts(actions: &[AliasHandle]) -> bool {
    actions.iter().enumerate().any(|(index, writer)| {
        actions[index + 1..].iter().any(|future| {
            matches!(
                (writer, future),
                (AliasHandle::Root, AliasHandle::Parent | AliasHandle::Child)
                    | (AliasHandle::Parent, AliasHandle::Child)
            )
        })
    })
}

fn alias_handle_id(handle: AliasHandle) -> &'static str {
    match handle {
        AliasHandle::Root => "write_root",
        AliasHandle::Parent => "write_parent",
        AliasHandle::Child => "write_child",
    }
}

fn decode_projected_handles(mut encoded: usize, len: u32) -> Vec<ProjectedHandle> {
    let mut actions = Vec::with_capacity(len as usize);
    for _ in 0..len {
        actions.push(if encoded.is_multiple_of(2) {
            ProjectedHandle::First
        } else {
            ProjectedHandle::Second
        });
        encoded /= 2;
    }
    actions
}

fn projection_sequence_conflicts(
    first: AliasProjection,
    second: AliasProjection,
    actions: &[ProjectedHandle],
) -> bool {
    if first != second {
        return false;
    }
    projected_sequence_uses_both_live(actions)
}

fn projected_sequence_uses_both_live(actions: &[ProjectedHandle]) -> bool {
    actions
        .iter()
        .enumerate()
        .any(|(index, writer)| actions[index + 1..].iter().any(|future| writer != future))
}

fn projected_handle_id(handle: ProjectedHandle) -> &'static str {
    match handle {
        ProjectedHandle::First => "write_first",
        ProjectedHandle::Second => "write_second",
    }
}

fn projection_id(projection: AliasProjection) -> &'static str {
    match projection {
        AliasProjection::Left => "left",
        AliasProjection::Right => "right",
    }
}

fn branch_order_id(order: BranchOrder) -> &'static str {
    match order {
        BranchOrder::DirectThenAlias => "direct_then_alias",
        BranchOrder::AliasThenDirect => "alias_then_direct",
    }
}

fn deep_projection_id(projection: DeepProjection) -> &'static str {
    match projection {
        DeepProjection::BranchLeft => "branch_left",
        DeepProjection::BranchRight => "branch_right",
        DeepProjection::OtherLeft => "other_left",
    }
}

fn deep_projection_source(projection: DeepProjection) -> &'static str {
    match projection {
        DeepProjection::BranchLeft => "tree.branch.left",
        DeepProjection::BranchRight => "tree.branch.right",
        DeepProjection::OtherLeft => "tree.other.left",
    }
}

fn loop_order_id(order: LoopOrder) -> &'static str {
    match order {
        LoopOrder::AliasThenDirect => "alias_then_direct",
        LoopOrder::DirectThenAlias => "direct_then_alias",
    }
}

fn index_projection_id(projection: IndexProjection) -> &'static str {
    match projection {
        IndexProjection::ConstantZero => "constant_0",
        IndexProjection::ConstantOne => "constant_1",
        IndexProjection::DynamicI => "dynamic_i",
        IndexProjection::DynamicJ => "dynamic_j",
    }
}

fn index_projection_source(projection: IndexProjection) -> &'static str {
    match projection {
        IndexProjection::ConstantZero => "0",
        IndexProjection::ConstantOne => "1",
        IndexProjection::DynamicI => "i",
        IndexProjection::DynamicJ => "j",
    }
}

fn expected_word(rejected: bool) -> &'static str {
    if rejected {
        "rejection"
    } else {
        "acceptance"
    }
}

fn origin_id(origin: ReferenceOrigin) -> &'static str {
    match origin {
        ReferenceOrigin::Parameter => "parameter",
        ReferenceOrigin::Unproven => "unproven",
    }
}

fn returned_arguments_id(returned: ReturnedArguments) -> &'static str {
    match returned {
        ReturnedArguments::Left => "returns_left",
        ReturnedArguments::Right => "returns_right",
        ReturnedArguments::Both => "returns_both",
    }
}

fn lifetime_projection_id(projection: LifetimeProjection) -> &'static str {
    match projection {
        LifetimeProjection::Field => "field",
        LifetimeProjection::Index => "index",
    }
}

fn lifetime_projection_declarations(projection: LifetimeProjection) -> &'static str {
    match projection {
        LifetimeProjection::Field => "struct Holder:\n    view: &[u8]\n",
        LifetimeProjection::Index => "struct Slots:\n    items: [&[u8]; 2]\n",
    }
}

fn lifetime_projection_parameter(projection: LifetimeProjection) -> &'static str {
    match projection {
        LifetimeProjection::Field => "holder: Holder",
        LifetimeProjection::Index => "slots: Slots",
    }
}

fn lifetime_projection_expr(projection: LifetimeProjection) -> &'static str {
    match projection {
        LifetimeProjection::Field => "holder.view",
        LifetimeProjection::Index => "slots.items[0]",
    }
}

fn origin_expr(origin: ReferenceOrigin, parameter: &str, unproven: &str) -> String {
    match origin {
        ReferenceOrigin::Parameter => parameter.to_string(),
        ReferenceOrigin::Unproven => format!("unknown_borrow({unproven})"),
    }
}

fn render_alias_source(actions: &[AliasHandle]) -> String {
    let mut body = String::new();
    for (index, action) in actions.iter().enumerate() {
        let binding = match action {
            AliasHandle::Root => "xs",
            AliasHandle::Parent => "parent",
            AliasHandle::Child => "child",
        };
        body.push_str(&format!("    {binding}[0] = {index}\n"));
    }
    format!(
        "fn check(xs: &mut [u8]) -> void:\n    let parent: &mut [u8] = xs\n    let child: &mut [u8] = parent\n{body}"
    )
}

fn render_projection_source(
    first: AliasProjection,
    second: AliasProjection,
    actions: &[ProjectedHandle],
) -> String {
    let mut body = String::new();
    for (index, action) in actions.iter().enumerate() {
        let binding = match action {
            ProjectedHandle::First => "first",
            ProjectedHandle::Second => "second",
        };
        body.push_str(&format!("    {binding}[0] = {index}\n"));
    }
    format!(
        "struct Pair:\n    left: &mut [u8]\n    right: &mut [u8]\n\nfn check(pair: Pair) -> void:\n    let first: &mut [u8] = pair.{}\n    let second: &mut [u8] = pair.{}\n{body}",
        projection_id(first),
        projection_id(second)
    )
}

fn render_branch_source(
    initial: AliasProjection,
    reassigned: AliasProjection,
    direct: AliasProjection,
    order: BranchOrder,
) -> String {
    let binding = |projection| match projection {
        AliasProjection::Left => "xs",
        AliasProjection::Right => "ys",
    };
    let direct_write = format!("    {}[0] = 1\n", binding(direct));
    let alias_write = "    alias[0] = 2\n";
    let suffix = match order {
        BranchOrder::DirectThenAlias => format!("{direct_write}{alias_write}"),
        BranchOrder::AliasThenDirect => format!("{alias_write}{direct_write}"),
    };
    format!(
        "fn check(flag: bool, xs: &mut [u8], ys: &mut [u8]) -> void:\n    let alias: &mut [u8] = {}\n    if flag:\n        alias = {}\n{suffix}",
        binding(initial),
        binding(reassigned)
    )
}

fn render_deep_projection_source(
    first: DeepProjection,
    second: DeepProjection,
    actions: &[ProjectedHandle],
) -> String {
    let mut body = String::new();
    for (index, action) in actions.iter().enumerate() {
        let binding = match action {
            ProjectedHandle::First => "first",
            ProjectedHandle::Second => "second",
        };
        body.push_str(&format!("    {binding}[0] = {index}\n"));
    }
    format!(
        "struct Leaves:\n    left: &mut [u8]\n    right: &mut [u8]\n\nstruct Tree:\n    branch: Leaves\n    other: Leaves\n\nfn check(tree: Tree) -> void:\n    let first: &mut [u8] = {}\n    let second: &mut [u8] = {}\n{body}",
        deep_projection_source(first),
        deep_projection_source(second)
    )
}

fn render_loop_source(alias: AliasProjection, direct: AliasProjection, order: LoopOrder) -> String {
    let binding = |projection| match projection {
        AliasProjection::Left => "xs",
        AliasProjection::Right => "ys",
    };
    let alias_write = "        alias[0] = 1\n";
    let direct_write = format!("        {}[0] = 2\n", binding(direct));
    let body = match order {
        LoopOrder::AliasThenDirect => format!("{alias_write}{direct_write}"),
        LoopOrder::DirectThenAlias => format!("{direct_write}{alias_write}"),
    };
    format!(
        "fn check(xs: &mut [u8], ys: &mut [u8]) -> void:\n    let alias: &mut [u8] = {}\n    for i in 0 .. 2:\n{body}",
        binding(alias)
    )
}

fn render_index_projection_source(first: IndexProjection, second: IndexProjection) -> String {
    format!(
        "struct Slots:\n    items: [&mut [u8]; 2]\n\nfn fill(left: &mut [u8], right: &mut [u8]) -> void:\n    return\n\nfn check(slots: Slots, i: usize, j: usize) -> void:\n    fill(slots.items[{}], slots.items[{}])\n",
        index_projection_source(first),
        index_projection_source(second)
    )
}

fn render_branch_in_loop_source(
    initial: AliasProjection,
    reassigned: AliasProjection,
    direct: AliasProjection,
    order: LoopOrder,
) -> String {
    let binding = |projection| match projection {
        AliasProjection::Left => "xs",
        AliasProjection::Right => "ys",
    };
    let alias_write = "        alias[0] = 1\n";
    let direct_write = format!("        {}[0] = 2\n", binding(direct));
    let body = match order {
        LoopOrder::AliasThenDirect => format!("{alias_write}{direct_write}"),
        LoopOrder::DirectThenAlias => format!("{direct_write}{alias_write}"),
    };
    format!(
        "fn check(flag: bool, xs: &mut [u8], ys: &mut [u8]) -> void:\n    let alias: &mut [u8] = {}\n    for iteration in 0 .. 2:\n        if flag:\n            alias = {}\n{body}",
        binding(initial),
        binding(reassigned)
    )
}

fn render_direct_lifetime_source(origin: ReferenceOrigin, alias_depth: usize) -> String {
    let mut body = String::new();
    let mut current = origin_expr(origin, "xs", "xs");
    for depth in 0..alias_depth {
        let name = format!("alias_{depth}");
        body.push_str(&format!("    let {name}: &[u8] = {current}\n"));
        current = name;
    }
    body.push_str(&format!("    return {current}\n"));
    format!("fn check(xs: &[u8]) -> &[u8]:\n{body}")
}

fn render_conditional_lifetime_source(left: ReferenceOrigin, right: ReferenceOrigin) -> String {
    let left = origin_expr(left, "xs", "xs");
    let right = origin_expr(right, "ys", "ys");
    format!(
        "fn check(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:\n    if flag:\n        return {left}\n    return {right}\n"
    )
}

fn render_call_lifetime_source(
    returned: ReturnedArguments,
    left: ReferenceOrigin,
    right: ReferenceOrigin,
    helper_alias: bool,
    caller_alias: bool,
) -> String {
    let helper_return = |name: &str, indent: &str| {
        if helper_alias {
            format!("{indent}let selected: &[u8] = {name}\n{indent}return selected\n")
        } else {
            format!("{indent}return {name}\n")
        }
    };
    let helper_body = match returned {
        ReturnedArguments::Left => helper_return("left", "    "),
        ReturnedArguments::Right => helper_return("right", "    "),
        ReturnedArguments::Both => format!(
            "    if flag:\n{}{}",
            helper_return("left", "        "),
            helper_return("right", "    ")
        ),
    };
    let left = origin_expr(left, "xs", "xs");
    let right = origin_expr(right, "ys", "ys");
    let call = format!("select(flag, {left}, {right})");
    let caller_body = if caller_alias {
        format!("    let result: &[u8] = {call}\n    return result\n")
    } else {
        format!("    return {call}\n")
    };
    format!(
        "fn select(flag: bool, left: &[u8], right: &[u8]) -> &[u8]:\n{helper_body}\nfn check(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:\n{caller_body}"
    )
}

fn render_match_lifetime_source(left: ReferenceOrigin, right: ReferenceOrigin) -> String {
    let left = origin_expr(left, "xs", "xs");
    let right = origin_expr(right, "ys", "ys");
    format!(
        "enum Choice:\n    Left\n    Right\n\nfn check(choice: Choice, xs: &[u8], ys: &[u8]) -> &[u8]:\n    match choice:\n        case Choice::Left:\n            return {left}\n        case Choice::Right:\n            return {right}\n"
    )
}

fn render_loop_lifetime_source(
    kind: &str,
    body: ReferenceOrigin,
    fallback: ReferenceOrigin,
) -> String {
    let body = origin_expr(body, "xs", "xs");
    let fallback = origin_expr(fallback, "ys", "ys");
    let loop_stmt = if kind == "for" {
        format!("    for i in 0 .. 1:\n        return {body}\n")
    } else {
        format!("    while flag:\n        return {body}\n")
    };
    format!(
        "fn check(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:\n{loop_stmt}    return {fallback}\n"
    )
}

fn render_wrapper_lifetime_source(
    returned: ReturnedArguments,
    left: ReferenceOrigin,
    right: ReferenceOrigin,
    depth: usize,
) -> String {
    let base_body = match returned {
        ReturnedArguments::Left => "    return left\n".to_string(),
        ReturnedArguments::Right => "    return right\n".to_string(),
        ReturnedArguments::Both => {
            "    if flag:\n        return left\n    return right\n".to_string()
        }
    };
    let mut source =
        format!("fn select(flag: bool, left: &[u8], right: &[u8]) -> &[u8]:\n{base_body}\n");
    let mut callee = "select".to_string();
    for layer in 0..depth {
        let wrapper = format!("wrap_{layer}");
        source.push_str(&format!(
            "fn {wrapper}(flag: bool, left: &[u8], right: &[u8]) -> &[u8]:\n    return {callee}(flag, left, right)\n\n"
        ));
        callee = wrapper;
    }
    let left = origin_expr(left, "xs", "xs");
    let right = origin_expr(right, "ys", "ys");
    source.push_str(&format!(
        "fn check(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:\n    return {callee}(flag, {left}, {right})\n"
    ));
    source
}

fn render_direct_projection_lifetime_source(
    projection: LifetimeProjection,
    alias_depth: usize,
) -> String {
    let mut body = String::new();
    let mut current = lifetime_projection_expr(projection).to_string();
    for depth in 0..alias_depth {
        let alias = format!("alias_{depth}");
        body.push_str(&format!("    let {alias}: &[u8] = {current}\n"));
        current = alias;
    }
    body.push_str(&format!("    return {current}\n"));
    format!(
        "{}\nfn check({}) -> &[u8]:\n{body}",
        lifetime_projection_declarations(projection),
        lifetime_projection_parameter(projection)
    )
}

fn render_conditional_projection_lifetime_source(
    projection: LifetimeProjection,
    projection_first: bool,
) -> String {
    let projected = lifetime_projection_expr(projection);
    let (first, second) = if projection_first {
        (projected, "xs")
    } else {
        ("xs", projected)
    };
    format!(
        "{}\nfn check(flag: bool, xs: &[u8], {}) -> &[u8]:\n    if flag:\n        return {first}\n    return {second}\n",
        lifetime_projection_declarations(projection),
        lifetime_projection_parameter(projection)
    )
}

fn render_call_projection_lifetime_source(
    projection: LifetimeProjection,
    projection_selected: bool,
) -> String {
    let projected = lifetime_projection_expr(projection);
    let (left, right) = if projection_selected {
        (projected, "xs")
    } else {
        ("xs", projected)
    };
    format!(
        "{}\nfn select_left(left: &[u8], right: &[u8]) -> &[u8]:\n    return left\n\nfn check(xs: &[u8], {}) -> &[u8]:\n    return select_left({left}, {right})\n",
        lifetime_projection_declarations(projection),
        lifetime_projection_parameter(projection)
    )
}

fn render_self_recursive_lifetime_source(
    base: ReferenceOrigin,
    recursive_arg: ReferenceOrigin,
    alias: bool,
) -> String {
    let base = origin_expr(base, "xs", "xs");
    let recursive_arg = origin_expr(recursive_arg, "xs", "xs");
    let (setup, base_return, recursive_call_arg) = if alias {
        (
            format!("    let base: &[u8] = {base}\n    let next: &[u8] = {recursive_arg}\n"),
            "base",
            "next",
        )
    } else {
        (String::new(), base.as_str(), recursive_arg.as_str())
    };
    format!(
        "fn recurse(flag: bool, xs: &[u8]) -> &[u8]:\n{setup}    if flag:\n        return {base_return}\n    return recurse(flag, {recursive_call_arg})\n"
    )
}

fn render_mutual_recursive_lifetime_source(origin: ReferenceOrigin, alias: bool) -> String {
    let origin = origin_expr(origin, "xs", "xs");
    let (setup, base_return) = if alias {
        (
            format!("    let candidate: &[u8] = {origin}\n"),
            "candidate",
        )
    } else {
        (String::new(), origin.as_str())
    };
    format!(
        "fn left(flag: bool, xs: &[u8]) -> &[u8]:\n{setup}    if flag:\n        return {base_return}\n    return right(flag, xs)\n\nfn right(flag: bool, xs: &[u8]) -> &[u8]:\n    if flag:\n        return xs\n    return left(flag, xs)\n"
    )
}

fn decode_actions(mut encoded: usize, len: u32) -> Vec<Action> {
    let mut actions = Vec::with_capacity(len as usize);
    for _ in 0..len {
        actions.push(ACTIONS[encoded % ACTIONS.len()]);
        encoded /= ACTIONS.len();
    }
    actions
}

fn build_case(scenario: Scenario) -> ReferenceMoveCase {
    let expected = evaluate(&scenario).use_after_move;
    let id = scenario_id(&scenario);
    ReferenceMoveCase {
        description: format!(
            "independent move-state model expects {} for {}",
            if expected { "rejection" } else { "acceptance" },
            id.strip_prefix("reference.moves.").unwrap_or(&id)
        ),
        source: render_source(&scenario),
        id,
        expects_use_after_move: expected,
    }
}

fn evaluate(scenario: &Scenario) -> MoveState {
    match scenario {
        Scenario::Linear(actions) => actions
            .iter()
            .fold(MoveState::initial(), |state, action| state.apply(*action)),
        Scenario::Branch {
            left,
            right,
            suffix,
        } => MoveState::initial()
            .apply(*left)
            .join(MoveState::initial().apply(*right))
            .apply(*suffix),
        Scenario::Loop { body, suffix } => MoveState::initial()
            .apply(*body)
            .apply(*body)
            .apply(*suffix),
    }
}

fn scenario_id(scenario: &Scenario) -> String {
    match scenario {
        Scenario::Linear(actions) => format!(
            "reference.moves.linear.{}",
            actions.iter().map(action_id).collect::<Vec<_>>().join("_")
        ),
        Scenario::Branch {
            left,
            right,
            suffix,
        } => format!(
            "reference.moves.branch.{}_{}_{}",
            action_id(left),
            action_id(right),
            action_id(suffix)
        ),
        Scenario::Loop { body, suffix } => format!(
            "reference.moves.loop.{}_{}",
            action_id(body),
            action_id(suffix)
        ),
    }
}

fn action_id(action: &Action) -> &'static str {
    match (action.access, action.place) {
        (Access::Read, Place::Root) => "read_root",
        (Access::Move, Place::Root) => "move_root",
        (Access::Read, Place::Left) => "read_left",
        (Access::Move, Place::Left) => "move_left",
        (Access::Read, Place::Right) => "read_right",
        (Access::Move, Place::Right) => "move_right",
    }
}

fn render_source(scenario: &Scenario) -> String {
    let mut body = String::new();
    let mut binding = 0usize;
    match scenario {
        Scenario::Linear(actions) => {
            for action in actions {
                render_action(&mut body, *action, 4, &mut binding);
            }
        }
        Scenario::Branch {
            left,
            right,
            suffix,
        } => {
            body.push_str("    if flag:\n");
            render_action(&mut body, *left, 8, &mut binding);
            body.push_str("    else:\n");
            render_action(&mut body, *right, 8, &mut binding);
            render_action(&mut body, *suffix, 4, &mut binding);
        }
        Scenario::Loop {
            body: action,
            suffix,
        } => {
            body.push_str("    for i in 0 .. 2:\n");
            render_action(&mut body, *action, 8, &mut binding);
            render_action(&mut body, *suffix, 4, &mut binding);
        }
    }
    format!(
        "struct Packet:\n    value: u8\n\nstruct Pair:\n    left: Packet\n    right: Packet\n\nfn check(flag: bool, pair: Pair) -> void:\n{body}"
    )
}

fn render_action(out: &mut String, action: Action, indent: usize, binding: &mut usize) {
    let padding = " ".repeat(indent);
    match (action.access, action.place) {
        (Access::Read, Place::Root) => {
            out.push_str(&format!(
                "{padding}let read_left_{}: u8 = pair.left.value\n",
                *binding
            ));
            *binding += 1;
            out.push_str(&format!(
                "{padding}let read_right_{}: u8 = pair.right.value\n",
                *binding
            ));
        }
        (Access::Read, Place::Left) => out.push_str(&format!(
            "{padding}let read_left_{}: u8 = pair.left.value\n",
            *binding
        )),
        (Access::Read, Place::Right) => out.push_str(&format!(
            "{padding}let read_right_{}: u8 = pair.right.value\n",
            *binding
        )),
        (Access::Move, Place::Root) => out.push_str(&format!(
            "{padding}let moved_root_{}: Pair = pair\n",
            *binding
        )),
        (Access::Move, Place::Left) => out.push_str(&format!(
            "{padding}let moved_left_{}: Packet = pair.left\n",
            *binding
        )),
        (Access::Move, Place::Right) => out.push_str(&format!(
            "{padding}let moved_right_{}: Packet = pair.right\n",
            *binding
        )),
    }
    *binding += 1;
}

#[cfg(test)]
mod tests {
    use super::{
        generated_alias_cases, generated_lifetime_cases, generated_move_cases,
        generated_safety_campaign, generated_safety_property_cases,
        DEFAULT_SAFETY_CAMPAIGN_CASES_PER_FAMILY, DEFAULT_SAFETY_CAMPAIGN_SEED,
    };

    #[test]
    fn independent_move_model_generates_the_stable_exhaustive_matrix() {
        let cases = generated_move_cases();
        assert_eq!(cases.len(), 510);
        assert!(cases.iter().any(|case| {
            case.id == "reference.moves.linear.move_left_read_right" && !case.expects_use_after_move
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.moves.branch.move_left_read_left_read_left"
                && case.expects_use_after_move
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.moves.loop.move_right_read_left" && case.expects_use_after_move
        }));
    }

    #[test]
    fn independent_alias_model_generates_the_stable_exhaustive_matrix() {
        let cases = generated_alias_cases();
        assert_eq!(cases.len(), 277);
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.linear.write_child_write_parent"
                && !case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.linear.write_parent_write_child"
                && case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.linear.write_root_write_parent"
                && case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.projection.left_right.write_first_write_second"
                && !case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.projection.left_left.write_first_write_second"
                && case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.branch.left_to_right.right_direct_then_alias"
                && case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.branch.left_to_left.right_direct_then_alias"
                && !case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.deep_projection.branch_left_branch_left.write_first_write_second"
                && case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.deep_projection.branch_left_branch_right.write_first_write_second"
                && !case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.loop.left_left.alias_then_direct"
                && case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.loop.left_right.direct_then_alias"
                && !case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.index.constant_0_constant_1"
                && !case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.index.dynamic_i_dynamic_j"
                && case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.branch_in_loop.left_to_right.left_alias_then_direct"
                && case.expects_mutable_borrow_alias
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.aliases.branch_in_loop.left_to_left.right_direct_then_alias"
                && !case.expects_mutable_borrow_alias
        }));
    }

    #[test]
    fn independent_lifetime_model_generates_the_stable_matrix() {
        let cases = generated_lifetime_cases();
        assert_eq!(cases.len(), 132);
        assert!(cases.iter().any(|case| {
            case.id == "reference.lifetimes.direct.parameter_aliases_2"
                && !case.expects_escaping_reference
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.lifetimes.conditional.parameter_unproven"
                && case.expects_escaping_reference
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.lifetimes.call.returns_left_parameter_unproven_helper_alias_true_caller_alias_true"
                && !case.expects_escaping_reference
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.lifetimes.call.returns_both_parameter_unproven_helper_alias_false_caller_alias_false"
                && case.expects_escaping_reference
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.lifetimes.match.parameter_unproven"
                && case.expects_escaping_reference
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.lifetimes.loop.for_parameter_parameter"
                && !case.expects_escaping_reference
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.lifetimes.loop.while_unproven_parameter"
                && case.expects_escaping_reference
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.lifetimes.wrapper.returns_left_parameter_unproven_depth_3"
                && !case.expects_escaping_reference
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.lifetimes.projection.direct.field_aliases_2"
                && case.expects_escaping_reference
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.lifetimes.projection.call.index_unselected"
                && !case.expects_escaping_reference
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.lifetimes.recursive.self.base_parameter_recursive_parameter_alias_false"
                && case.expects_escaping_reference
        }));
        assert!(cases.iter().any(|case| {
            case.id == "reference.lifetimes.recursive.mutual.unproven_alias_true"
                && case.expects_escaping_reference
        }));
    }

    #[test]
    fn seeded_safety_properties_are_stable_and_replayable() {
        let first = generated_safety_property_cases();
        let second = generated_safety_property_cases();
        assert_eq!(first, second);
        assert_eq!(first.len(), 256);
        for prefix in [
            "reference.properties.moves.",
            "reference.properties.aliases.",
            "reference.properties.projections.",
            "reference.properties.lifetimes.",
        ] {
            assert_eq!(
                first
                    .iter()
                    .filter(|case| case.id.starts_with(prefix))
                    .count(),
                64
            );
        }
        assert!(first.iter().any(|case| {
            case.id == "reference.properties.moves.000"
                && case.expected_diagnostic == Some("use_after_move")
        }));
        assert!(first.iter().any(|case| {
            case.id == "reference.properties.lifetimes.063" && case.description.contains("depth=")
        }));
    }

    #[test]
    fn runtime_seeded_campaign_is_replayable_and_supplies_smaller_candidates() {
        let first = generated_safety_campaign(
            DEFAULT_SAFETY_CAMPAIGN_SEED,
            DEFAULT_SAFETY_CAMPAIGN_CASES_PER_FAMILY,
        );
        let second = generated_safety_campaign(
            DEFAULT_SAFETY_CAMPAIGN_SEED,
            DEFAULT_SAFETY_CAMPAIGN_CASES_PER_FAMILY,
        );
        assert_eq!(first, second);
        assert_eq!(first.len(), 128);
        assert_ne!(first, generated_safety_campaign(7, 32));
        for family in ["moves", "aliases", "projections", "lifetimes"] {
            let prefix =
                format!("reference.campaign.v1.{DEFAULT_SAFETY_CAMPAIGN_SEED:016x}.{family}.");
            assert_eq!(
                first
                    .iter()
                    .filter(|case| case.id.starts_with(&prefix))
                    .count(),
                32
            );
        }
        for case in &first {
            assert!(case.replay.contains("--campaign-seed 0x"));
            assert!(case.replay.contains("--campaign-cases 32"));
            assert!(case
                .shrink_candidates
                .iter()
                .all(|candidate| candidate.complexity < case.complexity
                    && candidate.expected_diagnostic == case.expected_diagnostic));
        }
        assert!(first.iter().all(|case| !case.shrink_candidates.is_empty()));
    }
}
