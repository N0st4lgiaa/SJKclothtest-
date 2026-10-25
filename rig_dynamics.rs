//! Dynamic joints: the chains a rig model's manifest names (`dynamic <joint>`, the
//! joints under that one, such as a tail, a coat or a cape) move on their own. Each
//! simulated joint is a point pulled by a spring towards where the animation puts it,
//! so it lags and bounces when the body moves, held at its bone's length from its
//! parent; a gentle side-to-side wag, growing towards the tip, keeps it alive while the
//! player stands, and gravity, more towards the tip, makes it hang (a tail's rest pose
//! sticks out straight; the game it comes from lets physics pull it down).
//!
//! Cloth adds three things, each off unless its `dynamic` line asks (so a tail moves as
//! it always did):
//!
//! - **Air** (`drag`, `inertia`): the player's run through the world. The chain is
//!   simulated in the model's space, which does not move when the player does, so the
//!   world's motion is handed in ([`State::observe`], called once a frame with where
//!   the player is and which way they face): a cape streams out behind a running
//!   player (`drag`, the air rushing past, plus the turn's sweep), and lags when they
//!   start, stop or swerve (`inertia`, the body's acceleration read backwards).
//! - **The body** (`collide`): capsules round the legs and trunk that the chain cannot
//!   be pushed into, so a coat does not cut through thighs. A `soft` capsule (the back a
//!   cape lies against) only keeps the chain from going deeper than the animation has it,
//!   so a cape is never pushed off the body it hangs on.
//! - **Per-chain feel** (`wag`, `stiffness`, `damping`): a coat wants no wag, a cape a
//!   softer spring than a tail.
//!
//! Then each parent is turned to point at its simulated child. The state is the
//! actor's ([`super::Scratch`]); the chain's own motion happens in the model's space,
//! so it answers the animation (steps, jumps, turns of the hips) and, through
//! [`Motion`], the air.

use super::Joint;
use glam::{Affine3A, Mat3A, Quat, Vec3A};

/// The spring pulling a joint towards its animated place, per second squared.
const STIFFNESS: f32 = 70.0;
/// How fast the swing dies down, per second.
const DAMPING: f32 = 7.0;
/// The wag at the chain's tip, game units to each side, and how many a second.
pub(super) const WAG_UNITS: f32 = 2.5;
const WAG_HERTZ: f32 = 0.7;
/// The pull down on a chain's tip, game units per second squared, unless its
/// `dynamic` line gives one: twice the game's (`g_gravity` 800), which only levelled
/// the test model's tail (its rest pose rises 22 degrees); this hangs it about 29 degrees
/// below level, the tip some 12 units under its anchor. Less towards the anchor.
pub(super) const GRAVITY: f32 = 1600.0;
/// A frame step longer than this (a hitch, a teleport, the first frame) starts the
/// chain again at the animated pose.
const MAX_STEP: f32 = 0.1;
/// The longest step the spring is integrated in, seconds: a frame is cut into steps this
/// short, so a stiff chain (`stiffness` high) stays stable at any frame rate.
const SUBSTEP: f32 = 1.0 / 90.0;

/// How the player's motion is smoothed, seconds: snapshots arrive in steps, and the
/// acceleration read off their differences would otherwise jolt at each one.
const SMOOTHING: f32 = 0.05;
/// A player further from where they were a frame ago than this (game units) has been
/// teleported or respawned, not run: the air forgets the old place.
const MAX_JUMP: f32 = 200.0;
/// Limits on what the air is told, so a dash or a knockback cannot throw a cape:
/// speed (units a second), acceleration (units a second squared), turning (radians a
/// second).
const MAX_SPEED: f32 = 1200.0;
const MAX_ACCELERATION: f32 = 4000.0;
const MAX_TURN: f32 = 12.0;

/// How one chain moves: the manifest's `dynamic <joint> [gravity] [key value]...` line,
/// its defaults filled in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Tuning {
    /// Units per second squared, pulling down (more towards the tip).
    pub(super) gravity: f32,
    /// Units each side at the tip.
    pub(super) wag: f32,
    /// The spring towards the animated place, per second squared.
    pub(super) stiffness: f32,
    /// How fast the swing dies down, per second.
    pub(super) damping: f32,
    /// How hard the air rushing past pushes the chain, per second (acceleration per unit
    /// of air speed): 0 is none, a cape wants a few.
    pub(super) drag: f32,
    /// How much of the body's acceleration the chain lags behind, 0 to 1 and more.
    pub(super) inertia: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            gravity: GRAVITY,
            wag: WAG_UNITS,
            stiffness: STIFFNESS,
            damping: DAMPING,
            drag: 0.0,
            inertia: 0.0,
        }
    }
}

/// A capsule the chains stay out of, between two joints' animated places, its radius in
/// game units (the manifest's `collide <joint> <end joint> <radius> [soft]`, glTF
/// metres scaled with the model).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Capsule {
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) radius: f32,
    /// Only keeps a point as far out as the animation has it (up to `radius`), where a
    /// hard capsule keeps it out to `radius` whatever the animation does.
    pub(super) soft: bool,
}

/// The simulated joints of a rig, parents before children.
#[derive(Clone, Debug, Default)]
pub(super) struct Chains {
    pub(super) joints: Vec<usize>,
    /// How far along its chain each one is, 0 at the anchor to 1 at the tip.
    along: Vec<f32>,
    /// Its chain's feel.
    tuning: Vec<Tuning>,
}

impl Chains {
    /// The joints under each of `anchors` (not the anchors: they follow the
    /// animation and hold the chain), in `order`, each anchor with its chain's
    /// tuning.
    pub(super) fn under(joints: &[Joint], order: &[usize], anchors: &[(usize, Tuning)]) -> Self {
        let below = |mut index: usize| -> Option<(usize, Tuning)> {
            let mut depth = 0;
            while let Some(parent) = joints[index].parent {
                depth += 1;
                if let Some((_, tuning)) = anchors.iter().find(|(anchor, _)| *anchor == parent) {
                    return Some((depth, *tuning));
                }
                index = parent;
            }
            None
        };
        let found: Vec<(usize, usize, Tuning)> = order
            .iter()
            .filter_map(|&index| below(index).map(|(depth, tuning)| (index, depth, tuning)))
            .collect();
        let deepest = found.iter().map(|(_, depth, _)| *depth).max().unwrap_or(1) as f32;
        Self {
            joints: found.iter().map(|(index, _, _)| *index).collect(),
            along: found
                .iter()
                .map(|(_, depth, _)| *depth as f32 / deepest)
                .collect(),
            tuning: found.iter().map(|(_, _, tuning)| *tuning).collect(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.joints.is_empty()
    }
}

/// The player's motion through the world, in the model's space (+X the player's side,
/// -Y their front, +Z up), smoothed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Motion {
    /// Units a second.
    pub(super) velocity: Vec3A,
    /// Units a second squared.
    pub(super) acceleration: Vec3A,
    /// Radians a second about +Z, positive turning left.
    pub(super) turn: f32,
}

/// Where the player was at the last [`State::observe`], in the world, smoothed.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seen {
    origin: Vec3A,
    yaw_degrees: f32,
    time_ms: i64,
    velocity: Vec3A,
    acceleration: Vec3A,
    turn: f32,
}

/// One actor's chain state.
#[derive(Clone, Debug, Default)]
pub(crate) struct State {
    points: Vec<Vec3A>,
    velocities: Vec<Vec3A>,
    /// Seconds simulated, for the wag.
    time: f32,
    /// The animated pose before the chain moved it (scratch).
    animated: Vec<Affine3A>,
    aimed: Vec<bool>,
    /// The player's motion, as last observed.
    pub(super) motion: Motion,
    seen: Option<Seen>,
}

impl State {
    /// Tell the chains where the player is in the world (`origin`, game units), which
    /// way their model faces (`yaw_degrees`, the legs' yaw the model is drawn with) and
    /// when (`time_ms`, the presented time). Call once a frame, before posing; the
    /// speed, acceleration and turning the air gives the chains come from the changes
    /// between calls. A repeated time (a paused demo) changes nothing; a rewind, a
    /// hitch or a teleport starts the air again at rest.
    pub(crate) fn observe(&mut self, origin: [f32; 3], yaw_degrees: f32, time_ms: i64) {
        let origin = Vec3A::from(origin);
        let fresh = Seen {
            origin,
            yaw_degrees,
            time_ms,
            velocity: Vec3A::ZERO,
            acceleration: Vec3A::ZERO,
            turn: 0.0,
        };
        let Some(seen) = self.seen else {
            self.seen = Some(fresh);
            return;
        };
        if time_ms == seen.time_ms {
            return;
        }
        let elapsed = (time_ms - seen.time_ms) as f32 / 1000.0;
        let shift = origin - seen.origin;
        if !(0.0..=MAX_STEP).contains(&elapsed) || shift.length() > MAX_JUMP {
            self.seen = Some(fresh);
            self.motion = Motion::default();
            return;
        }
        // Each reading is moved towards the new raw one by how much of the smoothing
        // time this step covers.
        let keep = (-elapsed / SMOOTHING).exp();
        let velocity = shift / elapsed * (1.0 - keep) + seen.velocity * keep;
        let acceleration =
            (velocity - seen.velocity) / elapsed * (1.0 - keep) + seen.acceleration * keep;
        let turned = wrap_degrees(yaw_degrees - seen.yaw_degrees).to_radians();
        let turn = turned / elapsed * (1.0 - keep) + seen.turn * keep;
        self.seen = Some(Seen {
            origin,
            yaw_degrees,
            time_ms,
            velocity,
            acceleration,
            turn,
        });
        // The model faces -Y and the entity's yaw is +X-forward: the world's axes turn by
        // the yaw and a quarter turn more to the model's (`weapon_view::actor_world_rotation`).
        let to_model = Quat::from_rotation_z(-(yaw_degrees + 90.0).to_radians());
        self.motion = Motion {
            velocity: to_model * velocity.clamp_length_max(MAX_SPEED),
            acceleration: to_model * acceleration.clamp_length_max(MAX_ACCELERATION),
            turn: turn.clamp(-MAX_TURN, MAX_TURN),
        };
    }
}

/// `degrees` brought into -180 to 180.
fn wrap_degrees(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

/// The point of the segment `from` to `to` nearest `at`.
fn nearest(from: Vec3A, to: Vec3A, at: Vec3A) -> Vec3A {
    let axis = to - from;
    let length_squared = axis.length_squared();
    if length_squared < 1e-9 {
        return from;
    }
    from + axis * ((at - from).dot(axis) / length_squared).clamp(0.0, 1.0)
}

/// Move `chains` in `pose` by `step` seconds (`None`: start again at the animated
/// pose), keeping them out of `colliders` (at their animated places in `pose`).
pub(super) fn simulate(
    joints: &[Joint],
    chains: &Chains,
    colliders: &[Capsule],
    pose: &mut [Affine3A],
    state: &mut State,
    step: Option<f32>,
) {
    let count = chains.joints.len();
    let step = step.filter(|step| *step > 0.0 && *step <= MAX_STEP);
    let restart = step.is_none() || state.points.len() != count;
    let step = step.unwrap_or(0.0);
    state.time += step;
    let phase = std::f32::consts::TAU * WAG_HERTZ * state.time;
    let motion = state.motion;
    let target = |slot: usize, pose: &[Affine3A]| {
        let along = chains.along[slot];
        // Model space: +X is the player's side (-Y the front, +Z up). The tip trails
        // the base a little, so the wag runs down the chain.
        pose[chains.joints[slot]].translation
            + Vec3A::X * chains.tuning[slot].wag * along * (phase - along * 0.6).sin()
    };
    // Gravity on each point, more towards the tip.
    let weight = |slot: usize| Vec3A::NEG_Z * chains.tuning[slot].gravity * chains.along[slot];
    if restart {
        // Already hanging: where the spring holds the weight. Not with colliders, which
        // push points out but cannot bring one back from the far side: the chain then
        // starts at the animated pose and settles from there.
        let settle = if colliders.is_empty() { 1.0 } else { 0.0 };
        state.points = (0..count)
            .map(|slot| target(slot, pose) + weight(slot) / chains.tuning[slot].stiffness * settle)
            .collect();
        state.velocities = vec![Vec3A::ZERO; count];
    } else {
        let substeps = ((step / SUBSTEP).ceil() as usize).max(1);
        let h = step / substeps as f32;
        for _ in 0..substeps {
            for slot in 0..count {
                let tuning = &chains.tuning[slot];
                let point = state.points[slot];
                let pull = target(slot, pose) - point;
                // The air at this point, against the body: the player's run, and the
                // sweep of a turn (the body carries the point round the yaw axis); the
                // body's acceleration read backwards. More towards the tip.
                let air = -(motion.velocity + Vec3A::new(-point.y, point.x, 0.0) * motion.turn);
                let wind =
                    (air * tuning.drag - motion.acceleration * tuning.inertia) * chains.along[slot];
                let velocity = &mut state.velocities[slot];
                *velocity +=
                    (pull * tuning.stiffness - *velocity * tuning.damping + weight(slot) + wind)
                        * h;
                state.points[slot] += *velocity * h;
            }
        }
    }
    // Each point at its bone's length from its parent's (simulated or animated), then
    // out of the body; twice, as a push out of a capsule can stretch the bone.
    for _ in 0..2 {
        for slot in 0..count {
            let index = chains.joints[slot];
            let Some(parent) = joints[index].parent else {
                continue;
            };
            let anchor = chains
                .joints
                .iter()
                .position(|&joint| joint == parent)
                .map_or(pose[parent].translation, |at| state.points[at]);
            let bone = pose[index].translation - pose[parent].translation;
            let towards = (state.points[slot] - anchor)
                .try_normalize()
                .or_else(|| bone.try_normalize())
                .unwrap_or(Vec3A::Z);
            state.points[slot] = anchor + towards * bone.length();
            // The animated place: against a soft capsule the cloth may lie as close to the
            // axis as the animation has it (a cape against a back), and no closer.
            let animated = pose[index].translation;
            for capsule in colliders {
                let (from, to) = (pose[capsule.from].translation, pose[capsule.to].translation);
                let beside = animated - nearest(from, to, animated);
                let allowed = if capsule.soft {
                    beside.length().min(capsule.radius)
                } else {
                    capsule.radius
                };
                let axis = nearest(from, to, state.points[slot]);
                let away = state.points[slot] - axis;
                if away.length() < allowed {
                    let outward = away
                        .try_normalize()
                        .or_else(|| beside.try_normalize())
                        .unwrap_or(Vec3A::X);
                    state.points[slot] = axis + outward * allowed;
                    // What the point was carrying into the capsule stops there: left to
                    // build, a hanging chain would tunnel through in one step.
                    let inward = state.velocities[slot].dot(outward).min(0.0);
                    state.velocities[slot] -= outward * inward;
                }
            }
        }
    }
    // Each parent turned to point at its simulated child; the children keep their
    // animated place under it.
    state.animated.clear();
    state.animated.extend_from_slice(pose);
    state.aimed.clear();
    state.aimed.resize(pose.len(), false);
    for slot in 0..count {
        let index = chains.joints[slot];
        let Some(parent) = joints[index].parent else {
            continue;
        };
        let local = state.animated[parent].inverse() * state.animated[index];
        if !state.aimed[parent] {
            let origin = pose[parent].translation;
            let now = ((pose[parent] * local).translation - origin).try_normalize();
            let wanted = (state.points[slot] - origin).try_normalize();
            if let (Some(now), Some(wanted)) = (now, wanted) {
                let turn = Mat3A::from_quat(Quat::from_rotation_arc(now.into(), wanted.into()));
                pose[parent].matrix3 = turn * pose[parent].matrix3;
            }
            state.aimed[parent] = true;
        }
        pose[index] = pose[parent] * local;
    }
}
