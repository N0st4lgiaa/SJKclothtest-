# Rig models

A rig model is a player model drawn from a glTF file with its own skeleton (any
number of joints, fingers, face, cloth), animated by the game's ordinary
`_humanoid` animations. Built 14/10/2026 ([status](status.md#rig-models-gltf-players-with-their-own-skeleton)).

## Why this design

Three ways were weighed on 14/10/2026 before building this:

- **Bake the model onto Kyle's GLM skeleton** (re-skin it to JKA's 53 bones). Works
  with every client, but loses fingers, face and any extra bone, and every model
  needs hand weighting.
- **Replace the skeleton outright** (animate the glTF with its own clips). Needs
  every JKA animation re-made for each skeleton, and the server, saber traces and
  bolts all speak `_humanoid` bone numbers.
- **Hybrid, the one built:** keep `_humanoid` as a hidden driver and retarget its pose
  onto the rig every frame. The server, protocol, traces and animation numbers stay
  untouched; any skeleton works through a bone map; joints the map leaves out stay at
  rest today and can play the rig's own clips later (emotes, face).

A profile before the work (world shot `animation_cost_on_a_demo`, release build, a
19-player demo) measured pose, skinning and upload at 0.064 ms a frame with vertex
skinning already on the GPU, so the rig path is about looks, not speed.

## How it works

The player keeps a stock **driver** model, Kyle unless the manifest names another.
Its GLM and `_humanoid.gla` do everything they did before: the server's animation
numbers, the split torso and legs, look angles, saber and effect bolts, saber traces,
Force effects, cosmetics. Only its surfaces are not drawn, and its bolts are moved
onto the rig (below).

The glTF mesh is drawn in their place. Every frame, after the driver's bone matrices
are evaluated ([actor_pose_steps.rs](../crates/sjk-viewer/src/actor_pose_steps.rs)),
[rig_model.rs](../crates/sjk-viewer/src/rig_model.rs) poses the rig:

- a **mapped** joint takes its JKA bone's model-space rotation change from the base
  pose. At load, its bind pose is first turned so its direction to a chosen child
  matches the JKA bone's direction to its own child (an A-pose rig fits JKA's base
  pose this way); `Rest` joints (head, feet) keep their bind orientation;
- the joint mapped to `pelvis` also follows the pelvis translation, so jumps,
  crouches and rolls move the body;
- **unmapped** joints (twist bones, ears, face, cloth) keep their bind pose under
  their parent.

The result is a joint palette skinned on the GPU like any player
([gpu_skinning.rs](../crates/sjk-viewer/src/gpu_skinning.rs)), or on the CPU when
`SJK_GPU_SKINNING=0`. Posing costs one affine multiply per joint per player and
allocates nothing per frame.

### Feet on the ground

`scale auto` matches pelvis heights, but a rig whose legs and feet reach further below
its pelvis than Kyle's sinks (the first test model sank 2.6 to 3.2 units standing,
walking and running, of about 70 units of height; 1 crouched). So every frame
`RigModel::pose` plants the feet: the driver's lower ankle (`ltalus`, `rtalus`)
gives the ground (minus how far its ankle stands above its sole in the base pose,
the driver GLM's lowest drawn vertex, [`sole`](../crates/sjk-viewer/src/rig_model.rs)),
and the whole rig moves up or down so its own lower ankle (the joints mapped to those
bones) stands as far above that ground as in its bind pose. A clip stands its bind
soles on the same ground. After it the same model is within a unit of Kyle's soles
in those poses (a clip within a third standing), the rest being how each foot's toes
and heel tilt. The manifest's `lift <units>` raises (or lowers) the result by hand;
a rig without mapped ankles keeps the pelvis placement alone.

### Dynamic joints (tails, coats, capes)

A manifest's `dynamic <joint>` lines make the joints under each named one move on
their own ([rig_dynamics.rs](../crates/sjk-viewer/src/rig_dynamics.rs), 14/10/2026):
each is a point pulled by a spring towards where the animation puts it (stiffness 70
per second squared, damping 7 per second), held at its bone's length from its parent,
and each parent is then turned to point at its simulated child. So a tail lags and
bounces when the body moves and settles behind it. A side-to-side wag of 2.5 units at
the tip, 0.7 times a second, running down the chain, keeps it alive while the player
stands. Gravity pulls each point down, more towards the tip: 1600 units per second
squared by default (twice the game's), or the line's own value (`dynamic <joint>
<gravity>`, 0 for none). A tail's rest pose sticks out straight (the game it comes
from lets physics pull it down): the test model's rose 29 degrees from level, too high in
the game; with the default it hangs 24 degrees below (measured with the ignored test
`dynamic_chains_of_an_installed_rig`, `SJK_TEST_PACK` naming the pack). The named joint stays where the animation holds it. It all happens in the
model's space, per actor (`Scratch`): the chain answers the animation (steps, jumps,
turns of the hips, emotes). A frame step over 0.1 s (a hitch, a teleport) starts the
chain again where the animation puts it.

A `dynamic` line takes `key value` pairs after the optional bare gravity, for a chain
that should feel like cloth rather than a tail (each key's default is the tail's):

- `wag <units>`: the side-to-side wag at the tip (2.5; 0 for a coat that hangs still).
- `stiffness <n>`, `damping <n>`: the spring (70 per second squared) and how fast the
  swing dies down (7 per second). A softer spring (lower stiffness) swings wider.
- `drag <n>`: the air rushing past. The player's motion through the world is read
  each frame from where they are and which way they face (`Scratch::observe`, called
  from `actor_pose_steps.rs`), smoothed over 0.05 s, and pushes the chain the other
  way, more towards the tip: a cape streams out behind a running player and sweeps
  out in a turn. 0 (the default) is none; the tip is held out by about
  `drag x speed / stiffness` units, so a cape wants a few.
- `inertia <n>`: how much of the body's acceleration the chain lags behind (0, none;
  1, all of it): the cloth hangs back when the player starts, stops or swerves.

`collide <joint> <end joint> <radius> [soft]` lines put capsules round the body that
the dynamic chains are kept out of, from one joint's animated place to another's;
`<radius>` is in glTF metres, so it follows `scale`. A hard capsule (a thigh) keeps a
chain out to the radius whatever the animation does, so a coat does not cut through a
kicking leg. A `soft` one (the back a cape lies against) only keeps a chain from going
deeper than the animation has it, so it can never push the cloth off the body it hangs
on. A chain starts at the animated pose, not already hanging, while any `collide` line
exists, so it cannot start on the far side of a capsule.

Example (a long coat and a cape on a mannequin-preset rig):

```
dynamic dyn_coat_bk_c_01 wag 0 drag 2 inertia 0.5
dynamic cape_l_MSTR wag 0 drag 5 inertia 0.5 stiffness 40
collide thigh_l calf_l 0.09
collide calf_l foot_l 0.07
collide pelvis spine_03 0.16 soft
```

### Bolts on the rig

The driver's bolts (`*r_hand`, `*l_hand`, `*head_top`, the feet, cosmetics, the
jetpack's `*chestg`) and the Force effect points (hands, head, lumbar for a carried
flag) sit where Kyle's bones put them, which misses a rig with other proportions.
`PlayerPreview::bolt` and `RigModel::retarget_bolt` move each one onto the rig: the
bolt keeps the driver's orientation (mapped joints turn as the driver's bones do)
and its offset from its bone, stretched by the limb's length ratio along the map's
`Toward` pair, but starts from the joint mapped to that bone (or to its nearest
mapped ancestor). A saber then sits in the rig's hand, a hat on its head. Saber
traces and hit boxes stay the driver's.

### Code map

| Step | Where |
| --- | --- |
| Folder with `sjk_rig.cfg` found, driver loaded instead of the folder's GLM | `load_player_appearance` in [player_assets.rs](../crates/sjk-viewer/src/player_assets.rs) |
| glTF read (skin, joints, primitives, images) | [gltf.rs](../crates/sjk-model/src/gltf.rs) |
| Manifest, presets, fit (axes, scale, alignments, anchors), pose, CPU skin | [rig_model.rs](../crates/sjk-viewer/src/rig_model.rs), tests in `rig_model_tests.rs` |
| Rig surfaces put in the shared vertex buffer | `append_rig_mesh` in [actor_load.rs](../crates/sjk-viewer/src/actor_load.rs) |
| Per-frame pose and palette upload (GPU and CPU paths) | [actor_pose_steps.rs](../crates/sjk-viewer/src/actor_pose_steps.rs), `rig_skin_records` in [gpu_skinning.rs](../crates/sjk-viewer/src/gpu_skinning.rs) |
| Dynamic joints (tails, cloth) | `Chains`, `State::observe` and `simulate` in [rig_dynamics.rs](../crates/sjk-viewer/src/rig_dynamics.rs), `RigModel::pose_after` |
| Feet planted on the driver's ground | `Feet`, `sole` and `RigModel::with_feet` in [rig_model.rs](../crates/sjk-viewer/src/rig_model.rs), the ignored test `feet_against_the_driver` |
| Bolts and Force points moved onto the rig | `PlayerPreview::bolt`, `RigModel::retarget_bolt` and `retarget_point`, [force_bones.rs](../crates/sjk-viewer/src/force_bones.rs) |
| Dismemberment skipped, JoF first-person body off | [dismember.rs](../crates/sjk-viewer/src/dismember.rs), [actor_world_submission.rs](../crates/sjk-viewer/src/actor_world_submission.rs) |
| Clips (emotes): read, converted, sampled, blended | [rig_clips.rs](../crates/sjk-viewer/src/rig_clips.rs), `RigModel::pose` and `play`, `emotes::playing` in [emotes.rs](../crates/sjk-viewer/src/emotes.rs) |
| Body fit: limbs out of wider stock bodies | `Body`, `push`, `fit_limb` in [rig_body.rs](../crates/sjk-viewer/src/rig_body.rs), `Fit` in [rig_stock.rs](../crates/sjk-viewer/src/rig_stock.rs) |
| Clips played on stock GLM models (the bone map in reverse) | `StockEmotes` in [rig_stock.rs](../crates/sjk-viewer/src/rig_stock.rs), `stock_emotes` in [player_assets.rs](../crates/sjk-viewer/src/player_assets.rs), [actor_pose_steps.rs](../crates/sjk-viewer/src/actor_pose_steps.rs) |
| Blender export | [scripts/blender/sjk_rig_export](../scripts/blender/sjk_rig_export/__init__.py) |

Units and axes are converted at load: glTF metres with +Y up and +Z front become the
GLA's model space (+Z up, -Y front, as a Blender export of a model facing -Y). The
scale fits the rig's pelvis height to the driver's (`scale auto`).

## The manifest: `sjk_rig.cfg`

A model folder (`models/players/<name>/`) with `sjk_rig.cfg` is a rig model; `/model
<name>` picks it like any other. One `key value` per line, `//` comments:

| Line | Meaning | Default |
| --- | --- | --- |
| `model <file>` | the glTF, relative to the folder: `.gltf` with its `.bin` and images, or `.glb` | `model.gltf`, else `model.glb` |
| `driver <folder>` | the stock model whose skeleton drives the rig | `models/players/kyle` |
| `preset <name>` | the bone map to start from ([`PRESETS`](../crates/sjk-viewer/src/rig_model.rs)) | `mannequin` |
| `scale <n>` or `scale auto` | game units per glTF metre | `auto` |
| `lift <units>` | raise (negative: lower) the planted rig by hand | `0` |
| `dynamic <joint> [gravity] [key value]...` | the joints under it move on their own (spring, wag, gravity in units per second squared, 1600 when left out), one line per chain; keys `gravity`, `wag`, `stiffness`, `damping`, `drag`, `inertia` | none |
| `collide <joint> <end joint> <radius> [soft]` | a capsule (radius in glTF metres) the dynamic chains stay out of | none |
| `map <joint> <bone>` | the joint follows the JKA bone, bind orientation kept | |
| `map <joint> <bone> <joint end> <bone end>` | ... aimed along the bone | |
| `map <joint> <bone> as <other joint>` | ... with another joint's alignment (finger chains) | |

`map` lines replace the preset's entry for that joint or add one.

### Presets

- `mannequin`: Unreal Engine mannequin names (`pelvis`, `spine_01`..`spine_05`,
  `neck_01`, `neck_02`, `head`, `clavicle_l`, `upperarm_l`, `lowerarm_l`, `hand_l`,
  `thumb_01_l`..`pinky_03_l`, `thigh_l`, `calf_l`, `foot_l`, and `_r`). Five spine
  joints share JKA's three; JKA's two-joint finger groups (thumb `d1`, index and
  middle `d2`, ring and pinky `d4`) drive all three joints of each finger. Clavicles
  are left to the spine.

Another skeleton naming is a new preset entry, not code elsewhere.

## The glTF

Read by [gltf.rs](../crates/sjk-model/src/gltf.rs): the first skin, its joints'
bind poses from the inverse bind matrices, and the triangle primitives of the meshes
that skin deforms; up to eight influences, of which the strongest four are kept.
Triangles are turned to the game's winding at load (glTF fronts are
counter-clockwise, the game's default `cull front` keeps clockwise ones; unturned,
the inside of the far side shows through). Each primitive's base colour texture
becomes its shader (the image's path, so a
`.shader` script of that name can override it); a primitive without one is drawn
`*white`. Not read: animations, morph targets (shape keys), normal and PBR maps,
images embedded in a buffer, data URIs.

## From Blender: the SJK Rig Model Export add-on

[scripts/blender/sjk_rig_export](../scripts/blender/sjk_rig_export/__init__.py) is a
Blender extension (4.2 or later) that writes a model, and the emotes picked for it,
in one click.

- **Install:** zip the folder's two files (`__init__.py`, `blender_manifest.toml`),
  then Edit > Preferences > Get Extensions > Install from Disk. The preferences take
  the GameData folder; left empty, the usual Steam and retail places are tried.
- **Use:** 3D view sidebar (N), SJK tab. Pick the armature; the panel counts the
  bone map's joints it has (`pelvis` and `hand_r` are required) and the meshes it
  deforms, and lists the actions that animate it. Tick the actions to bring as
  emotes (the round arrow beside one makes it loop), name the model and press Export
  to SJK.
- **What it writes:** by default one file, `<GameData>/base/rig_<name>.pk3`, holding
  `models/players/<name>/` (`model.gltf`, `model.bin`, the colour images,
  `sjk_rig.cfg`) and `emotes/<id>.emote` for each ticked action (the id is the
  emote's name, `Dance Moves` giving `dance_moves`, else `<name>_<action>`; two models
  in one pack should not give two emotes one name); deleting
  that file removes everything. "One .pk3 file" off writes the same as loose folders
  under `base`. "Pack name" (empty: `rig_<name>`) lets several models share one pack:
  an export replaces only its own model's files there (its folder, and the emote
  files, sounds and pictures of the animations its old glTF held, each an emote id),
  and warns when another pack in `base` also holds the model. An existing `sjk_rig.cfg` (in the pk3 or the folder) is kept, so `map`
  lines added by hand survive a re-export; a checkbox rewrites it. glTF Separate, +Y
  up, skins on, the armature's rest position, no shape keys, four influences. Other
  modifiers are not applied, except Mask modifiers shown in the viewport (geometry
  masked out is left out). Shape keys are dropped but their current mix is kept in
  the mesh: a key switched on (a top fitted over the chest, stockings fitted into
  shoes) stays; before 15/10/2026 the export fell back to the basis shape and such
  clothes showed the body through.
- **Character icon:** a picture (`.png`, `.jpg`, `.tga`) packed as
  `models/players/<name>/icon_default.<ext>`, which puts the model in the character
  grid as `<name>/default` (`character_catalog.rs` lists a folder holding
  `sjk_rig.cfg` once it has that icon).
- **Following meshes:** a mesh rigged to another armature whose bone follows one of
  this armature's (a Child Of or Copy Transforms constraint, as a tail or a cape from
  an Unreal export is set up) goes with the model: a temporary copy of the armature
  is exported in its place, with the other armature's bones added under the bone it
  follows where they stand on the rest pose, and the mesh as it stands there with its
  own weights. The other armature's root bones are written as `dynamic` lines
  (replaced at each export; a `dynamic` line naming one of the armature's own bones,
  such as a tail added by hand, is kept), so the game moves them on their own, with the
  panel's Tail gravity (written when it is not the game's 1600). The panel counts
  them.
- **Height adjustment:** written as the manifest's `lift` line (0 removes it), the
  one manifest line the add-on changes in a kept manifest.
- **Emote settings:** each ticked action takes a sound (`.wav` or `.mp3`, packed as
  `sound/emotes/<id>.<ext>`); "Animation frame rate" (0 is the scene's; 30 for
  animations from Unreal games) sets the clip's speed; "Keep playing while moving"
  (on) writes `moving`; "Emotes stay on this computer" (off) writes `local`. Each
  takes a name (what the game and the wheel call it; empty: the action's name) and an
  icon (a picture, packed as `gfx/emotes/<id>.<ext>` and named by the file's `icon`,
  shown on the quick wheel).
- **Emotes:** the exporter's own animation export is not used (its NLA mode exports
  every track, muted ones too, and its action filter only exists once its file
  browser has opened). The add-on plays each ticked action alone (NLA off), samples
  its animated bones at every frame and appends them to the glTF: a joint node's
  transform is its rest (as the exporter wrote it) times the pose bone's basis,
  which holds because the exporter keeps child bones in Blender's bone space and
  puts the Y-up turn on the root bone's rest. Times come from the scene's frame
  rate the setting gives.
- **Materials:** each becomes a plain Principled BSDF with the image its base colour
  most likely comes from: an image node labelled `SJK` (unconnected is fine: the way
  to hand the game a baked texture, such as a colour with its tints applied or with
  the alpha of another image, without changing the Blender look), else the nearest image upstream of the Principled BSDF's Base
  Color, through the nodes between (Ambient Occlusion, Mix, Hue/Saturation; their
  tint is not carried over), else an image node feeding a socket named Base Color,
  Diffuse or Albedo, else one labelled so, else an image named `_D`, `_BaseColor`,
  `_Albedo`..., else the first sRGB image. A material with none gets a 4x4 image of
  its colour (Principled Base Color, else viewport colour, alpha included) named
  `<material>_color.png`; before 15/10/2026 it was drawn `*white` (black glasses
  frames came out white). Transparency, glow and two-sided cards are not read from
  Blender: a `shaders/<name>.shader` in the pack, its entries named after the
  textures (`models/players/<name>/<image>` without the extension), adds them, and
  re-exports keep it. Before the upstream walk (15/10/2026) an image behind an AO node lost to
  the first sRGB image, which on a hair material was the cyan alpha variant.
- **The blend file is untouched:** the meshes and materials exported are temporary
  copies, removed afterwards; the pose mode, selection, active action, NLA switch and
  frame come back as they were. The only things kept are the action's emote settings
  (`sjk_emote`, `sjk_emote_loop`, `sjk_emote_sound`, `sjk_emote_name`,
  `sjk_emote_icon`) and the scene's panel settings.
- **From a script:** `bpy.ops.sjk.export_rig_model("EXEC_DEFAULT")` exports the
  scene's settings; Blender in the background (`blender -b file.blend --python ...`)
  works, without the add-on's preferences (GameData is then guessed).

By hand instead: glTF Separate (`.gltf` + `.bin` + textures), +Y up, skinning on,
animations and shape keys off, the armature in its rest pose, each material a
Principled BSDF whose Base Color is an Image Texture.

Use it with your own files on your own disk. SJK never uploads, hosts or passes on
models taken from other games ([the rule](../AGENTS.md#verification-and-scope)).

## Limits

- Dismemberment is off for rig models: their glTF has none of the driver surfaces a
  cut hides and shows.
- In first person, JoF's body view (the saber arm drawn) does not show a rig model's
  body: it is one surface, with no head to hide. The classic first person applies.
- Menus (Character, Profile) still show the driver model; the character grid lists a
  rig model with an `icon_default` picture under "High quality", and picking it
  wears it.
- Only the player sees it: other players see the rig model only when they have the
  same folder; otherwise they see their own fallback for an unknown model, as for any
  custom model.
- The body fit reads each body as ellipsoids round the trunk bones and thighs: a cape,
  a backpack or big shoulder pads are not in it, and armour pieces passing through each
  other are the model's own skinning, which no fit changes.
- On a stock model an emote moves only the bones the bone map names: JKA has no
  third finger joint, twist bones or face bones in the map, so those parts follow
  their parent. Its feet are not planted (the pelvis stands on the driver's ankle
  height), so a stock model whose legs bend unlike the rig's can stand a little into
  or above the floor during a dance.
- Feet are planted vertically only: with legs of other proportions they can slide a
  little sideways, and a foot can stand slightly into or above a slope (no foot
  IK).
- The add-on's export of a 177-joint mannequin character matched a hand export
  (joints, primitives, images) and rendered in the world shot below (14/10/2026).
- First tried in the game by Sol on 14/10/2026: drawn and animated; the faces
  showed through (winding) and the saber missed the hand, both fixed the same day.

## Pitfalls

What went wrong or nearly did, so the next change does not repeat it:

- **Winding.** glTF fronts are counter-clockwise, the game's clockwise. Unturned, the
  model looked see-through (the inside of the far side drawn). Fixed by swapping two
  indices per triangle at load; a test pins it.
- **Bolts stay on the driver.** Anything read through `surface_bolt_matrix` on the
  driver GLM lands where Kyle's bones are: the saber floated beside the paw. Player
  bolts go through `PlayerPreview::bolt`; a new player bolt use must too (the menu
  stage still reads the driver directly, which is fine while menus show the driver).
- **What the driver's matrices are.** The per-bone matrices are skinning matrices
  (posed times inverse base pose, the matrix that moves a base-pose vertex); a bone's
  posed position is that matrix applied to its base-pose translation.
- **JKA's vertical offset.** A GLA frame's pose sits about 27 units below the base
  pose (the entity origin is mid-body). The engine needs nothing for it (the rig
  follows the pelvis exactly as Kyle does), but an offline preview in Blender must
  measure the pelvis against the standing idle or the feet sink.
- **Axes.** glTF is +Y up, +Z front; the GLA +Z up, -Y front; a Blender export of a
  model facing -Y lines up. A model facing another way needs turning before export.
- **Sinking feet.** Matching pelvis heights is not matching legs: a model sank about
  3 units until the feet were planted on the driver's every frame (measure with the
  ignored test `feet_against_the_driver`, JKA_GAME_DATA and SJK_TEST_RIG).
- **Scale.** `scale auto` divides the driver's pelvis height by the rig's; a rig whose
  pelvis joint sits at the origin needs `scale <n>`.
- **Emotes cleared outside a server.** The per-frame emote hook used to empty the
  active emotes whenever no live server was joined, so nothing played in a demo or a
  world shot; demos now keep theirs.
- **Exporter animation modes.** Blender's glTF exporter in NLA mode exports every
  track (muted ones too); the add-on samples actions itself instead.
- **Several joints on one bone** (`spine_01` and `spine_02` both on `lower_lumbar`):
  fine for posing; for bolts, a bone's first map entry is its anchor.

## Recipes

### Rerun the checks

```sh
# Rig and glTF unit tests (sjk-viewer has no lib target, hence --bins):
cargo test -p sjk-viewer --bins rig_model
cargo test -p sjk-model gltf
# Off-screen render of a rig model on a recorded game; 12 close-ups in
# target/world-shots/rig_model_orbit.png:
JKA_GAME_DATA=<GameData> SJK_TEST_DEMO=<demo.dm_26> SJK_TEST_RIG=<model folder name> \
  cargo test -p sjk-viewer --bins rig_model_on_a_demo -- --ignored --nocapture --test-threads=1
# The same with an emote of the installed catalogue playing (a rig clip); 12 shots
# about a third of a second apart in target/world-shots/rig_model_emote.png:
#   add SJK_TEST_EMOTE=<emote id>
# Host cost of animating a demo's players (release build for real numbers):
JKA_GAME_DATA=<GameData> SJK_TEST_DEMO=<demo.dm_26> \
  cargo test --release -p sjk-viewer --bins animation_cost_on_a_demo -- --ignored --nocapture
```

The render puts the rig on the first other player of the demo's first snapshot,
through the mid-match model change path. Look at the head and chest (nothing
see-through) and the right hand (the hilt in it).

### Support another skeleton naming

1. Add a function returning its `MapEntry` list and a `PRESETS` line in
   [rig_model.rs](../crates/sjk-viewer/src/rig_model.rs) (see `mannequin()`): each
   mapped joint names its JKA bone and, where it has a child to aim along, the
   `toward` pair; finger chains use `Align::As` for their second and third joints.
2. Add the same joints to `PRESET_JOINTS` and `PRESET_ITEMS` in the add-on (only
   used to tell the user whether a skeleton fits; keep the two in step).
3. Check it in Blender beside posed Kyle (next recipe), then in a world shot.

JKA's bone names and base poses: the `gla_dump` example below writes them.

### Preview a bone map in Blender

1. Dump some animations:
   `cargo run -p sjk-model --example gla_dump -- <_humanoid.gla> <animation.cfg> jka_anims.json BOTH_STAND1 BOTH_RUN1 BOTH_A2_T__B_`
   (the GLA and `animation.cfg` come out of the retail `assets1.pk3`, never into git).
2. Optional reference, posed Kyle as OBJ files to import beside the rig:
   `cargo run -p sjk-model --example glm_pose_obj -- <model.glm> <_humanoid.gla> <animation.cfg> <model_default.skin> <out dir> BOTH_STAND1:0 BOTH_RUN1:5`.
3. In Blender, make the rig's armature active, open
   [jka_retarget_preview.py](../scripts/blender/jka_retarget_preview.py) in the text
   editor, set `JSON`, edit `MAP` if the naming is not the mannequin's, run it, then
   call `run()`: each animation becomes a `JKA_<name>` action on the armature.
4. Compare the poses with Kyle's OBJs. The script's map mirrors `mannequin()`.

### Make a test model

With the add-on (above), from any rigged character you have the rights to. Keep test
models local: never commit them, attach them to issues or post them (the rule in
[AGENTS.md](../AGENTS.md#verification-and-scope)). Then in the game `/model <name>`,
third person, and the debug panel's `[rig-models]` checks.

## Emotes: the rig's own clips

Built 14/10/2026 on the [emotes path](hub-chat.md#emotes-in-the-client): a rig model
plays its glTF's animations on its own skeleton.

- [gltf.rs](../crates/sjk-model/src/gltf.rs) reads each animation's translation,
  rotation and scale channels that target skin joints (linear and step; cubic
  splines keep their key values), and every joint's rest transform (its node's own).
- [rig_clips.rs](../crates/sjk-viewer/src/rig_clips.rs) turns them into clips in the
  game's axes and units at load, samples a joint's local transform at a time (rest
  where a channel is missing) and fades: 0.25 s in from the driver's pose, 0.25 s out
  before the emote ends; a loop wraps.
- `RigModel::pose` poses the retargeted skeleton, then the clip's (each joint under
  its parent, a root joint under what its parent nodes add, `Joint::above`), shifted
  by how far the driver's current animation lowers the pelvis from its base pose
  (the GLA's offset), and blends the two per joint.
- An emote file names its clip: `{"name": "Dance Moves", "clip": "dance_moves",
  "length_ms": 3400, "local": true}`. `emotes::playing(slot)` gives the pose step the
  clip, its time and what is left; a player whose model lacks the clip keeps the
  driver's pose.
- **Shared between models** (14/10/2026): an emote file's `rig` names the model
  folder its clip is in (the add-on writes it). When a rig model loads, it borrows
  the installed emotes' clips made on other models (`RigModel::borrow_clips`): the
  source model is read without its meshes (kept for the session, `SOURCES`) and each
  clip is baked onto this skeleton once (`RigModel::borrowed`), keyed as the source
  is. Joints are found by name, so it works between skeletons of one naming, as
  Unreal mannequin characters (male, female, other builds) share. Each found joint
  turns from its own bind pose as the source's turns from the source's, in model
  space (so bone rolls may differ, bind postures should match: A-pose with A-pose);
  it keeps its own bone length, except the root and the joint under it (Unreal's
  root and pelvis), which move as the source's do. Joints the source lacks (a tail,
  extra hair) follow their parent. A rig that has fewer than 60% of the joints a
  clip moves does not borrow it (logged). Checked by unit tests and world shots of
  each test model playing the other's emote (14/10/2026). Stock (GLM) models play
  them too, through the bone map in reverse ([below](#emotes-on-stock-models)).
- Other SJK players on the server see an emote through the hub, which relays only
  its id: their client plays it when it has the same emote and model (each player's
  own files; SJK passes none on). `local` emotes play on this client only and are
  never sent. Either way `sjkemote` starts the player's own at once, and the hub's
  echo of it is not started again.
- `sound` plays on the player (JKA's music channel, so it cuts no voice or swing)
  and is stopped when the emote is cut short (replaced, stopped, the player leaving);
  one that runs its length lets the sound finish. Another player's follows them (the
  snapshot moves their sound source); the player's own plays in both ears, as their
  own entity is not in the snapshot to move it (until 14/10/2026 it stayed where it
  started). Settings > Sound > Emotes: Emote volume (`s_emoteVolume`, 2 by default,
  0 to 3) multiplies it over the effects volume; Attack ends emote
  (`cg_emoteAttackStop`, off by default) makes an attack or alt attack stop your own
  emote for everyone; Typing keeps emote (`cg_emoteTypingKeep`, on) keeps a typing
  player's emote until they move after; alt-tabbing out always silences an emote's
  sound ([hub chat](hub-chat.md#emotes-in-the-client)).
- The mixer has 128 voices (32 until 15/10/2026, as retail) and, when they are all
  playing, gives up the oldest sound for a new one, except an emote's: voices on the
  music channel go only when nothing else is left (`MixerConfig::protected_channel`).
  Before, a long emote track was always the oldest voice, so a busy fight cut it.
- `moving` keeps the emote going while the player runs and jumps (the clip replaces
  the whole body, so the dance slides along). `sjkemote stop`, also the quick wheel's
  Stop emote, ends it; the id `stop` is reserved and relayed like any emote. `sjkemote stop all|<player>` and `sjkemote mute <player>` end or hide other players' emotes on this screen ([hub-chat.md](hub-chat.md#emotes-in-the-client)).
  `sjkemote sync [player]` (the wheel's Sync emote) joins another player's emote in
  step, sharing its music ([hub-chat.md](hub-chat.md#emotes-in-the-client)).
- Bolts follow the clip: `retarget_bolt` reads the actor's blended pose (kept in
  `PlayerPreview::rig_pose`, posed before the bolts each frame) and carries the
  joint's change from its retargeted pose, so the saber stays in a dancing hand.
- The quick wheel's editor lists the installed emotes under Emotes (Settings > Quick
  wheel); a pick is a custom choice running `sjkemote <id>`.
- An emote ends when its length is over or the player moves 24 units. Demos keep the
  emotes started on them (none arrive from the hub there).

### Emotes on stock models

Built 15/10/2026 (Creyon, at Sol's request): a player wearing a stock model (Kyle, the
retail characters, community GLM models on `_humanoid`) plays the emotes made on rig
models, on the model's own bones ([rig_stock.rs](../crates/sjk-viewer/src/rig_stock.rs)).

- When the appearance loads (`load_player_appearance`), `StockEmotes::load` takes the
  installed emotes that name a `clip` and its `rig`, loads each source rig without its
  meshes fitted to the model's own skeleton (the same `SOURCES` cache borrowing uses)
  and keeps the ones whose bone map drives its bones. Nothing is baked: the clip is
  posed on the source rig each frame.
- Each JKA bone that a map entry names (the first one, as for bolts) takes the
  model-space turn its joint makes from its aligned bind pose: the joint's rotation
  times the inverse of `Driven::rotation`, which is `RigModel::place` read backwards.
  So an A-pose rig's dance turns JKA's arms from their base pose into the rig's.
- A mapped bone's base position is carried by the bone of its joint's nearest mapped
  ancestor in the rig, not by its GLA parent: JKA hangs the fingers and hang tags on
  the forearm (`rradius`), so a finger carried by its GLA parent stayed where the
  forearm held it while the dancing hand turned away (the broken hands of the first
  build, 15/10/2026). Every bone keeps the stock model's own lengths.
- Bones no entry names (clavicles, twist bones, face, the saber's hang tags) move
  rigidly with the bone the skeleton's own animations keep them most rigid with,
  among the mapped bones and their parent (`rigidity`: 256 of the GLA's frames,
  three points on each bone read back in each other bone's frame, the least variance
  wins; measured once per skeleton for the session). A root bone keeps the driver's
  matrix.
- The pelvis moves as the clip's pelvis moves from its bind position, times the stock
  legs' length over the rig's (pelvis height above the lower ankle, `ltalus` and
  `rtalus` against the joints mapped to them), from where the driver's pelvis is now;
  its height is measured from the driver's current lower ankle, so a crouch or a bob
  stays on the floor.
- The clip's pose fades over the driver's per bone (0.25 s in and out, as on a rig)
  and replaces the driver's matrices for drawing (GPU palette or CPU skin), bolts,
  Force points, cosmetics and the jetpack ([actor_pose_steps.rs](../crates/sjk-viewer/src/actor_pose_steps.rs)):
  the saber stays in the dancing hand. The trace LOD (saber traces) keeps the
  driver's pose, as for rig models. The server and its animation numbers are untouched.
- Other SJK players see it when they have the same emote and its source rig model
  (each player's own files), as for rig models.

Checked by the unit tests `a_stock_model_plays_a_rig_clip_on_its_own_bones`,
`a_stock_pelvis_follows_the_clips_pelvis_and_carries_its_children` and
`stock_bones_are_posed_parents_first` in `rig_model_tests.rs`, and
`a_stock_finger_stays_on_the_turning_hand` in `rig_stock_tests.rs`. The ignored test
`retail_animations_come_back_through_a_stock_emote` (`SJK_TEST_GLA`,
`SJK_TEST_ANIMATIONS`) builds a mannequin-named rig on the retail skeleton, plays
retail animations (standing, a roll, a saber attack, a back flip, a wall run) on it,
makes them its clip and plays that on the stock skeleton: every skinned bone comes
back within 3.4 units of where the animation puts it, measured from the pelvis
(15/10/2026, JA+ `_humanoid.gla`); the rest is JKA's shoulder shrug, which its
clavicle adds and the rig's spine carries here. Before the carrier fix the fingers
strayed 5.2 units and the right hang tag 5.1.

### Body fit: limbs out of wider bodies

Built 15/10/2026 (Creyon): a dance made on a slim rig and played on a wide stock model (a
heavy alien, a big coat, a turtle) put the hands and elbows where the slim body's surface
was, inside the wide one ([rig_body.rs](../crates/sjk-viewer/src/rig_body.rs)).

- Each skeleton's body is measured once from its own mesh: the stock model's GLM (first
  LOD, drawn surfaces) when its stock emotes load, the source rig's glTF in its bind pose
  when it is loaded to lend its clips (its surfaces are dropped afterwards). Each vertex
  counts for the JKA bone that moves it most (a rig vertex through its strongest joint's
  bone map entry). An ellipsoid is fitted round the vertices of each trunk bone and each
  thigh (`PARTS`: the middle 90% on each axis, so straps, holsters and capes do not widen
  it); what hangs under the arms, the shins and the neck is not trunk (`REGIONS`). Each
  forearm's and shin's thickness is the distance from its bone that 75% of its vertices
  are within (`LIMBS`).
- Each frame, after the clip's stock pose, each limb's elbow (knee), mid point and wrist
  (ankle) is measured against the parts on both skeletons: clearance from the part's
  surface along its radius, less the limb's thickness, less the base pose's own overlap
  (a mesh whose limbs lie in its ellipsoids at rest sets its own zero). A leg is not
  measured against its own thigh.
- A stock point nearer its body than the rig's is to its own, up to 2 units (`KEEP`),
  and never inside, is pushed out along the part's radius; a rig limb touching its body
  (within 1.5 units, fading out by 3) draws the stock limb onto its surface by up to 4
  units, so a hand on the hip stays on a wider hip. Further out the stock limb only has to
  stay out, so a stance (legs apart, legs crossed, arms out) keeps its shape. Per point,
  the deepest push wins; without one, the pull onto the part the rig touches.
- The pushes become an elbow and a wrist target: the upper bone turns about the
  shoulder (hip) and the lower about the new elbow (knee), both lengths kept, the elbow
  on the side it was pushed to, and every bone hung under them (fingers, hang tags, the
  saber) turns with them. Three rounds a frame, as a push out of one part can lead
  into another.

Measured 15/10/2026 with the ignored test
`body_fit_keeps_wide_models_limbs_out_of_their_bodies` (`SJK_TEST_GLA`,
`SJK_TEST_ANIMATIONS`, `SJK_TEST_SLIM` and `SJK_TEST_WIDE` naming two GLMs;
`SJK_TEST_OBJ_DIR` writes the deepest frame of each animation before and after as OBJ
files): eight JKA animations (idle, taunt, gesture, crossed-arm talk, crouch, run, roll,
sit to stand) made a slim community model's rig clip and played on four community models
with the JA+ `_humanoid.gla`. The limb points' summed depth into the body, before and
after: a turtle 10.0 to 0.6, a cat 2.0 to 0.3, a toad 9.4 to 2.0 (deepest 3.8 to 1.6
units), a thin robot 1.2 to 1.2 (deepest 1.9 to 2.6 in one animation, where the pull onto a
touched part goes a little deep). Pictures of the crossed-arm talk on the toad and the
sit-to-stand on the turtle showed the forearms come out in front of the chest and a hand
come onto the knee rather than into it, crossed legs still crossed. The depths are read
against the same ellipsoids the fit uses, so they say the fit does what it means to; the
game test says whether it looks right. Not measured: retail models (Jan, Reelo).

Speed: a clip's times come from the frame rate it is exported at. Blender scenes
are often 60 fps while Unreal animations are made at 30; the first emote played
twice too fast until the add-on's frame rate was set to 30 (its 204 frames then last
6.8 s, the length of the music it came with).

Face: joints a clip animates move, face bones included (the first test model has
29: jaw, lips, brows, eyes, ears). Shape keys (morph targets) are not read or drawn:
that needs their vertex offsets uploaded and blended before skinning in
[gpu_skinning.wgsl](../crates/sjk-viewer/src/gpu_skinning.wgsl) and the glTF's
`weights` channels.

Not done: shape key animation, root motion (a clip that walks away snaps back when
it ends).

## Checks

Unit tests in [gltf.rs](../crates/sjk-model/src/gltf.rs) and
`rig_model_tests.rs`; the world shot `world_shot::rig_model_shots::rig_model_on_a_demo`
puts the rig model `SJK_TEST_RIG` names on a player of the demo `SJK_TEST_DEMO`.
The bone map was first checked in Blender beside posed Kyle with `gla_dump`,
`glm_pose_obj` and `jka_retarget_preview.py` (recipe above).
