# Design sketch: armor penetration & module damage (the ballistics simulator)

**Status: SPEC for the in-progress build (2026-06-27).** Decided in a design interview; being
implemented now, starting with an isolated sandbox (§11). Graduates to ADR(s) if it survives
contact — the `ballistics`-as-library-plugin + `shooting` split (§10) is the most ADR-worthy part.
Vocabulary from `.agents/CONTEXT.md` and `.agents/skills/codebase-design` (seam, depth, leverage).

This is the next vertical-slice mechanic after the gunner's sight (`design/gunner-sight.md`). It is
deliberately deep and physically-grounded; §9 names what is *out* of the first slice so the depth
doesn't masquerade as scope.

## 1. The kill model — crew is the only health

There is **no tank HP**. A tank is dead the instant it has **fewer than 2 living crew**. Everything
else is emergent and *repairable* — only ammunition is terminal. Three paths empty crew:

- **Direct hits** — a penetrator or spall fragment reaching a crewman.
- **Engine fire** — damages nearby crew slowly, by proximity, over time.
- **Ammo cookoff** — detonation of an ammunition volume; **instantly kills all crew**.

Module damage (engine, breech, optics, transmission, …) *never* kills the tank — it degrades
capability and can be repaired. Ammunition is the one exception.

## 2. The unified primitive — the ballistic volume

There is no real distinction between an "armor plate" and a "module": both are a **watertight solid
mesh + a material** (density/hardness → a *material factor*) that taxes a penetrator over the
line-of-sight distance through it. The Tiger's upper front plate is a thin, dense, high-cost slab;
the engine is a big low-density box that only stops a round if the path chews through enough of it.
Same primitive.

- A **module** = a ballistic volume that *also* carries function + state (engine, ammo, breech).
- **Crew** = a ballistic volume that can be incapacitated.
- Every module/crewman is a ballistic volume; not every ballistic volume is a module (bare plates
  have no function to lose). The march reads the *volume* layer (cost only); the consequence step
  reads the *module/crew* layer — "same geometry, consumed by type" (ADR-0008).

**Geometric thickness, not a slope coefficient.** Thickness is *measured* from the solid: the path
enters the front face and exits the back face, and the distance through the solid **is** the
line-of-sight thickness. Slope is free (a sloped plate is geometrically longer); no `cos` term.
Friend is modelling the Tiger's armor as solid volumes now.

**Convexity is not required; watertight/manifold is.** ADR-0008's convex constraint is a *physics
solver* requirement on the dynamic collision proxy. The armor layer is read by our **penetration
raycast** — a static-frame spatial query — and ray-vs-triangle works on any geometry. What replaces
convexity is **manifold/watertight**, so entry/exit faces pair cleanly and "inside the solid" is
well-defined.

## 3. The penetrator march — velocity is the source of truth

For a fixed projectile (mass, caliber, hardness constant), penetration is a monotonic, invertible
function of velocity (DeMarre, `pen ∝ vⁿ`). So **velocity is the stored state; penetration
*capability* is its derivative.** The march, per volume crossed:

1. `capability = f(mass, velocity)` — reference-mm this shell can defeat right now. **Mass is the
   primary driver** (sectional density / KE, `pen ∝ massᴹ·vⁿ`, M≈0.5); velocity secondary; both held
   in the shell, mass constant so velocity stays the stored state. Caliber is *not* here — it drives
   overmatch and spall hole-size, not raw penetration. (This is what separates a tank shell from a
   bullet at equal speed, and is the seed of the per-shell data struct.)
2. `cost = LOS_distance × material_factor` (× angle effects), also reference-mm.
3. If `capability > cost` → **perforate**: spend `cost`, invert `f` back to a reduced **residual
   velocity** (the Lambert–Jonas shape — barely-penetrate exits slow, big overmatch barely slows),
   and throw spall (§5).
4. Else → **embed** and stop.

Properties:

- **Modules tax the penetrator too** — they are volumes. A shell can run out of steam *inside* the
  tank after crossing the outer plate plus an engine block.
- **The path is a multi-segment world-space ray** that bends (normalization) and deflects
  (ricochet). A segment can leave one tank and strike another (skip off a glacis into a neighbour).
- **Deliberate omission:** *shatter* (penetration going non-monotonic at extreme velocity against
  hard armor) is out of the slice.

## 4. Boundary interaction — the decision tree at each face

When the path reaches a volume's face:

1. **Impact angle** = angle between path and surface normal at the hit point.
2. **Overmatch** — if caliber ≥ ~k× the volume's thickness *along its normal* at that point: suppress
   ricochet, normalize almost fully, punch through (cost still applies). The game's namesake, but
   **one modifier among many — not the centerpiece.** Don't over-build it.
3. **Ricochet** — if not overmatched and impact angle exceeds the **per-shell-type ricochet
   threshold** (a fixed constant for now; we have one shell): **deflect** — spawn a new path segment
   off the face, bleed velocity, do *not* enter. **No spall on ricochet** for now (spall is an
   exit/perforation event only — §5).
4. **Otherwise** — **normalize** (bend the path toward the normal), enter, and march the geometric
   LOS cost; perforate→exit (spall) or embed.

The *structure* is the design; the magnitudes (k, ricochet angle, normalization degrees, material
factors, the DeMarre exponent) are **live knobs the sandbox exists to tune** — built as adjustable
constants, not baked.

## 5. Spall — the exit cone (the primary crew-killer)

On every **perforation exit**, the volume emits a **consistent, fixed-shape cone** — *not* a
per-shell dynamically-derived spray (that read as confusing/inconsistent). Dense on-axis at the
source (point-blank behind the plate is a guaranteed hit), thinning with angle and distance, so
survival odds rise the further off-axis and the deeper a component sits.

- **Cone density = expected fragment-units per direction.**
- A **fragment is an energy packet** (superseded the original 1-HP/no-pen token — energy-packet cut
  2026-06-28). It carries a penetration value (RHA-mm) that bleeds with distance (drag); it deposits
  damage scaled by that energy, then **punches through thin volumes** (losing the cost it spends) or
  **stops in thick ones**. So **geometric shadowing** still holds for the engine block, but a thin
  bulkhead no longer fully protects what's behind it, and a strong on-axis fragment can exit the tank
  to reach another a few metres back (arriving weak). On-axis fragments are stronger (narrower, more
  penetrating) — the continuous form of WT's "more power ↔ narrower cone" groups.
- AP spalls at the exit of **every** plate it perforates → multiple spall events per shot, each
  rolling independently against nearby components.

**Locked v1 model (2026-06-28).** The cone's *shape* is fixed (symmetric, half-angle constant,
axis = the penetrator's **exit direction** — which already carries the normalization bend; obliquity
adds no skew); only its *fragment budget* (density) scales with the shot. The budget is the
**product of a body term and a shell term** — both must be present or there is no spall:

> `spall_budget ∝ cost_paid × v_res² × caliber`

- **Body term = `cost_paid` = LOS_distance × material_factor** — the material the round chewed, i.e.
  the *supply* of fragments. Thin/soft body → ~0; crew (≈0 factor) don't scab. (Resolves the §9 open
  tab: budget scales with residual energy *and* material, not either alone.)
- **Shell term = `v_res²`** (residual energy) — the *power* to throw them forward. Barely-through →
  v_res≈0 → ~0; overmatch with energy to spare → violent.
- This is why both extremes are weak: barely-through starves the shell term; thin/soft body starves
  the body term. Optimal = a well-sized plate perforated with energy to spare.
- `material_factor` doubles as the spall-supply proxy for v1. A dedicated **`spall_factor`**
  (brittleness, the physically-correct scabbing driver — ductile RHA resists, brittle armor/cast iron
  throws) is **noted as a later refinement**, not built yet.

**Energy-packet refinement (2026-06-28).** The total-energy budget above is *factored* into a
**count × per-fragment energy**, which is cleaner and is where shell-type variation will live:
- **count ∝ cost × caliber** (material supply + hole size — how *many* fragments).
- **per-fragment energy ∝ v_res² × (on-axis weight)** (the shot's push, concentrated on-axis — how
  *hard* each one is). Energy → both damage *and* penetration (RHA-mm), and bleeds with distance
  (fragment drag). Product still ≈ `cost × v_res² × caliber`, so the §5 budget is preserved.
This single change subsumes three threads: cone shape (on-axis weighting), fragment penetration
(energy-gated), and fragment drag (distance term). Shell types then vary the energy total + its
on-axis concentration + a fragment-penetration ceiling (see the shell-as-data direction in the
handoff). Other shell types (APHE/APDS/APFSDS/HE/…) stay deferred — AP is the populated point.
- **HE is a separate, penetration-independent mechanic** (later): a fuse triggers on minimum
  steel-equivalent thickness, then after a delay detonates into a much denser, wider cone — the
  penetrator is gone. Not modelled in this slice.

## 6. Component damage — HP per component, never per tank

Every component has its own **HP pool** (e.g. crewman 3, engine 10). A fragment deposits 1 unit; the
**main penetrator transiting** a module deposits many (scaled by the energy it spent crossing it).
This is *local function state*, not a global health bar — the kill condition is still crew < 2.

- **Crew are soft** (low HP) — the kill currency; a graze chips, a faceful of cone or a clean
  transit kills. (HP, not binary, so fire-over-time and near-misses have somewhere to accumulate.)
- **Modules are tougher and repairable** — one stray fragment scratches an engine; a faceful or a
  direct transit wrecks it.
- **Degraded performance (later):** checkpoints preferred (e.g. ≤50% HP → −x% power) over continuous
  `hp% = perf%`, for legibility. Tuning, not slice.

## 7. Crew — stations with a backfill hierarchy

Crew are not a counter; each crewman **is a station/function**: gunner (aim), loader (reload), driver
(move), commander (view/command). Capability is never owned by a module alone — it is **served by
whichever living crewman holds that station, at their effectiveness.** Stations **backfill**, the
commander being the universal (degraded) backup:

- Loader down → commander loads, slower.
- Gunner down → commander overrides the optic (modern), or the player falls back to the commander's
  third-person view (old). Lose the commander too and that view is gone.

**The view/control modes are themselves crew functions** — third-person = the commander's eyes out of
the cupola; the gunner's optic (`sight.rs`, in flight) = the gunner's station. So the crew system
will eventually **gate** sight/aim/driving rather than sit beside them. *Flag for the gunner-sight
work:* the optic and third-person toggle are crew-served capabilities, not unconditional.

## 8. Catastrophic & environmental

- **Ammunition** — each shell is modelled **individually** as a ballistic volume + HP. Firing
  **depletes** the stowage, so an emptier rack is a smaller target and less catastrophic (the real
  "empty your ready rack" play). A shell's HP → 0 = **cookoff** = all crew dead.
- **Fire** — an **engine hit by a direct penetrator** (not fragments) has an ignition chance. Fire
  does **not spread**; it does range-per-tick damage to nearby crew/components and can be **put out**
  (a crew repair action). A dedicated **fuel** volume comes later.

## 9. Open tabs (deferred, named so they aren't lost)

- Spall budget *driver* is settled (§5: `cost_paid × v_res² × caliber`); remaining tuning = caliber
  exponent (1 vs 2 = hole-area), an overall fragment cap, continuous vs coarse-tiered count, and
  whether to split out a dedicated `spall_factor` (brittleness) from `material_factor`.
- Ricochet threshold dependencies: pure angle now; later velocity/caliber-scaled.
- HE: fuse minimum-thickness trigger + delay + dense wide cone (penetration-independent).
- Numeric magnitudes (k overmatch ratio, ricochet angle, normalization degrees, material factors,
  DeMarre exponent) — tuned live in the sandbox.
- Data homes: ballistic-volume geometry on the model (watertight); material factor + shell specs in
  RON (ADR-0010 spirit — ADR-0010 already flags per-plate armor as a genuinely node-attached case);
  crew/module/station definitions TBD.
- Repair detail (who, how long, occupies which station).
- Player feedback / legibility — how the player reads *what happened* to their tank.

## 10. Architecture & the seam

- **`ballistics` — a library feature plugin** (ADR-0002): projectile spawn + integration + the
  world-space march + ballistic-volume cost + spall + HP deposit + inspection hooks. A **deep
  module**: a small interface (fire a shot; a ballistic volume registers itself) over a large hidden
  implementation. Consumed by *both* front-ends.
- **Split today's `shooting.rs`:** the ballistics/march moves into the shared `ballistics` module;
  `shooting` keeps only the game-specific **gun control** (fire-on-click, reload, recoil), feeding
  `ballistics`. Same mechanic, two triggers (player's gun; sandbox's camera).
- **The sandbox is a second binary in the same crate**, not a separate crate (perpetual sync
  burden) and not an in-game `AppState` (pollutes the shipping app with sandbox-only systems). It
  composes a *subset* of the library's feature plugins + one sandbox plugin. This matches `main.rs`'s
  own note that features live in `GamePlugin` so they can be mounted on an alternate App.

## 11. Armor sandbox v1 — the build

An isolated tool to develop and *tune* the march deterministically, decoupled from driving/aiming.

**Binary:** `src/bin/armor_sandbox.rs`, run with `cargo run --bin armor_sandbox`. App composition:

```
DefaultPlugins + PhysicsPlugins          // runtime + sim
+ spec::plugin + tank::plugin            // the target tank (reused as-is)
+ ballistics::plugin                     // the shared mechanic (new lib module)
+ <sandbox plugin>                       // camera-as-gun, free-fly, time, inspection
```
Deliberately **no** `driving`, `aim`, `camera`, `sight`, `shooting`.

**Sandbox plugin:**

- **Free-fly camera that *is* the gun** — WASD + Ctrl/Shift to float; the shell spawns at camera
  centre and fires straight down the view axis. Inspection camera and firing solution are one object.
- Keys to set **muzzle velocity** and cycle **shell type** (non-positional inputs).
- **Time controls** — pause / slow-motion / single-step.
- **Inspection draw** — the path segments, entry/exit points, per-volume cost, residual velocity at
  each stage, the spall cones, and HP deposited; the **last shot's path frozen** on screen to A/B an
  angle.

**v1 simplifications:** modules/crew are just *named ballistic volumes with HP* — no function, no
backfill, no fire, no cookoff, no HE. Those are §§7–8 and arrive after the march itself feels right.

**API discipline (AGENTS.md):** verify every Bevy 0.19 / avian3d 0.7 API against versioned docs
(`docs.rs/bevy/0.19.0`, `docs.rs/avian3d/0.7.0`, or the `v0.19.0` / `v0.7.0` tags) *before* writing
it. Do not write engine code from memory.

## 12. Model↔code binding contract (LOCKED, 2026-06-27)

The seam between the two parallel workstreams (model authoring vs the `ballistics` march). **Model
side owns geometry** (`.blend` / `.glb`); **code side owns the material/HP scalars** (RON). They do
not edit the same files. Authored to by the model handoff and bound to by the code.

- **Ballistic volumes are named nodes parented to their rig part** (Hull / Turret / Gun), inheriting
  its motion exactly like `*_Collider` (ADR-0008). Turret armor parents under `Turret`.
- **The RON `volumes` map (keyed by node name) is the source of truth** (updated 2026-06-28 —
  *composition, not prefixes*). A node is a ballistic volume **iff it is a key** in
  `<tank>.tank.ron`'s `volumes`. Each entry: `material_factor` (always — shell-resistance per metre)
  plus **optional facets** that layer roles on top; today the only facet is `hp` (present → a
  damageable `ComponentHealth`; absent → pure armour). Composition over a `kind` enum (§2): future
  consequences (cookoff, crew station, fire) add *more* optional facets → each its own ECS component,
  never a central enum. Role and resistance are **independent data**, so a steel barrel is a
  `Module_` with `material_factor: 1000` *and* an `hp`.
- **One naming convention, NOT parsed for behaviour** (updated 2026-06-28): every ballistic volume
  is `Ballistic_<part>` (e.g. `Ballistic_Hull_UFP`, `Ballistic_Engine`, `Ballistic_Gunner`,
  `Ballistic_Ammo_L_0`, `Ballistic_Gun_Barrel`). The old `Armor_/Module_/Crew_/Ammo_` role-prefixes
  are **gone** — role lives in the RON facets, so encoding it in the name too was a second source of
  truth. The single prefix only marks "this mesh is a hitbox" (vs visual skin / `*_Collider` / rig
  nodes — the model's mesh kinds split by *purpose*, not role) and powers the **drift lint** (a
  `Ballistic_*` node absent from `volumes` warns). The code reads the RON, never the prefix; every
  `volumes` key must have a matching node (asserted at bind).
- **Mesh:** watertight / **manifold** solids; convex *not* required (penetration is a raycast query,
  not the physics solver); non-rendering (the sandbox visualises them itself).
- **No numbers in the model.** `material_factor`, `hp`, and future facets live in RON keyed by node
  name (ADR-0010). Model = named manifold solids; code (RON) = all semantics.
- **Mesh kinds split by purpose, not role** (restructure 2026-06-28): the rig is a **skeleton of
  Empties** (`Hull`, `Turret`, `Gun`, `Gun_Barrel`, `Muzzle`, `Wheel_*`, `Track_*`, `Center_Of_Mass`
  — plain names, bound by the rig contract; their origin *is* the pivot). Every mesh is a **leaf**
  under a rig empty, prefixed by purpose: `Visual_*` (rendered skin, no gameplay), `Ballistic_*`
  (the march's hitboxes, concave OK), `Collider_*` (convex physics proxy, Vehicle layer). No mesh is
  ever the parent of another mesh or a rig node, so the art carries no mechanism and the three shapes
  stay independent. `Collider_*` is matched by prefix in `on_tank_ready`.

## Build status (2026-06-27)

- **Done & verified:** `shooting` → `ballistics` split (shared mechanic, trigger-agnostic `FireShell`
  event); `bin/armor_sandbox` + `sandbox.rs` — free-fly camera-gun (heading-relative WASD,
  Shift/Ctrl altitude), pause + 5-step slow-mo + `[`/`]` fine time control, shell tracer gizmo.
  **Geometric march cut:** `Layer::Armor` + `BallisticVolume`; the step march crosses volumes
  recording entry/exit (geometric line-of-sight thickness via a `solid=false` exit probe restricted
  to the same entity), stops at terrain, handles multiple volumes per shot; `PenetrationMarks`
  inspection gizmos (entry/exit/through-span). `cargo test` green.
- **Velocity cost cut:** `capability(speed) = K·speed^N` (DeMarre) and its inverse; a crossing spends
  `cost = LOS_metres × material_factor` and drops to the residual speed (Lambert–Jonas shape). When
  capability ≤ cost the shell **embeds** partway and stops. Placeholder plates now show
  perforate-then-embed. `0` joins `P` as a pause key.
- **Path bending cut:** `integrate_projectiles` is now a true ray-march carrying position +
  direction + speed, so bends survive across the step. **Normalization** straightens the round toward
  the inward normal on entry (a share of the incidence); **ricochet** deflects (specular, speed
  bled) off faces past ~70° without entering — the deflected segment lives in world space and can hit
  the next surface. Ricochet points drawn as cyan markers.
- **Information layer:** `ballistics` exposes a per-shell `ShellReadout` (speed, remaining
  capability); the sandbox draws a keybindings legend, a status line (time scale + shell count), and
  pooled labels floating beside each shell (speed / capability / plates crossed) via
  `world_to_viewport`. "Slower" is now a number.
- **Overmatch cut:** the shell carries a `caliber`; at each crossing the march probes the plate's
  thickness *along its normal* and, when `caliber ≥ 3 × thickness`, suppresses ricochet and nearly
  fully normalizes (cancelling slope). Sandbox plates are now a steel thickness ladder
  (15/50/100/300 mm); overmatched crossings draw magenta.
- **Real model bound:** `on_tank_ready` attaches a query-only trimesh collider (`Armor` layer,
  `filters = NONE`, so no physics response) + a `BallisticVolume` to each `Armor_/Module_/Crew_/Ammo_`
  node; the march resolves the volume by walking up from the hit mesh-primitive to the named parent
  (`ChildOf`). Material factor is **provisional, role-keyed** (`ballistics::material_factor`) pending
  RON authoring. The sandbox now spawns the real Tiger as a **static** target (reusing `on_tank_ready`
  via `spec`), alongside the placeholder slabs. Game + sandbox bind with no panic.
- **Spall + HP cut (2026-06-28):** per-component HP pools (`ComponentHealth`, role-keyed
  `component_hp` — crew 3 / module 10 / ammo 2; armor 0), bound to `Component` nodes in
  `on_tank_ready`. On every perforation exit the march throws a fixed-shape cone (symmetric, exit-dir
  axis, deterministic golden-angle fragment pattern denser on-axis); budget `= MAX × (cost/ref) ×
  (v_res/ref)² × (caliber/ref)` (§5 product model). Each fragment ray-casts to the first ballistic
  volume, deposits 1 HP, and stops (armor shadows for free). The main penetrator's transit deposits
  `cost × TRANSIT_K` into a crossed component (embed deposits `cap`). Sandbox draws spall cones +
  fragment rays (hot = HP deposited, grey = shadowed) and floats HP labels over damaged components;
  `c` resets component HP. `cargo test` green, sandbox runs clean. Constants are sandbox knobs.
- **Air drag cut (2026-06-28):** shells now bleed speed in flight — quadratic drag `dv/dt = −k·v²`
  integrated analytically (`v ← v/(1 + k·v·dt)`, stable), lumped const `DRAG_K` (sandbox-tunable). The
  point is **range-dependent penetration**: `capability ∝ vⁿ` now falls with distance, so a far shot
  can bounce where a near one perforates — visible live via the shell's speed/capability label. One AP
  value for now; the per-shell ballistic coefficient (the APCR-vs-APDS range-falloff differentiator)
  joins the shell-data struct later. Fragments stay hitscan — their drag will be a distance term in
  the future energy-packet model, not a flight integrator. Deliberately *not* doing full exterior
  ballistics (wind, altitude density, spin drift) — out of slice.
- **Energy-packet fragment cut (2026-06-28):** spall fragments upgraded from 1-HP/no-pen tokens to
  energy packets. `spall_directions` now returns each fragment's on-axis position `t`; count scales
  with `cost × caliber`, per-fragment birth penetration = `FRAG_PEN_MAX × shot_energy(v_res²) ×
  (1−t)`. New `cast_spall_fragment` marches each as a mini-penetrator: deposits `pen ×
  FRAG_DMG_PER_MM` HP on a component, then punches through thin volumes (spending `span × factor`) or
  stops in thick ones; `pen` bleeds with distance via `FRAG_DRAG`. Thick volumes still shadow; thin
  ones can be defeated; strong fragments can exit + reach another tank. Sandbox draw unchanged (reads
  end + deposited; rays now run longer when a fragment penetrates). Consts `FRAG_PEN_MAX/DRAG/
  DMG_PER_MM` are sandbox knobs. `cargo test` green, sandbox runs clean.
- **Mass-driven penetration cut (2026-06-28):** capability went from speed-only to
  `PEN_K · mass^MASS_EXP · speed^PEN_N` (MASS_EXP 0.5) — mass is the primary driver, caliber stays for
  overmatch/spall only. `FireShell`/`Projectile` carry `mass`; `capability`/`speed_for` take it.
  Re-calibrated `PEN_K` (0.01853→0.0058) so the 88 (10.2 kg @773) = 250 mm *unchanged* (zero
  regression); a 13 g MG round now lands ~9 mm RHA, so small arms can't defeat real armour but chip
  exposed modules. The linchpin for small-arms-vs-main-gun tiering and future shell types. `cargo
  test` green, sandbox clean.
- **Ricochet-shock cut (2026-06-28):** a ricochet off a *component* (not armor) now deposits shock
  damage `SHOCK_K · capability · cos(incidence)` — scaled by impact energy and squareness. So a
  grazing main-gun bounce chips an exposed module's integrity without one-shotting it (~3.7 of a 10-HP
  module at a 71° graze, ~0.4 at 88°), a bullet graze barely registers (~0.1), and armor (no HP) takes
  nothing. Completes the gun-barrel damage story (direct = kill, graze = chip, fragment = energy-
  scaled). `SHOCK_K` is a sandbox knob. Not yet drawn distinctly (HP labels show the effect).
- **Volume data → RON (composition) cut (2026-06-28):** retired the prefix-keyed `material_factor`/
  `component_hp` functions; the per-tank `volumes` RON map is now the source of truth. `VolumeSpec`
  = `material_factor` + optional `hp` facet (composition, not a `kind` enum — §12). `on_tank_ready`
  binds by iterating `spec.volumes` (node is a volume iff a key), `hp` present → `ComponentHealth`,
  absent → `ArmorVolume`; prefixes demoted to a drift lint; every RON key asserted to have a node.
  All 45 Tiger volumes authored + bound clean; resistance now decoupled from role (barrel = steel
  1000 *and* a module). Schema test covers it. **Consequences (§§7–8) now slot in as new facets.**
- **Single `Ballistic_` prefix cut (2026-06-28):** retired the four `Armor_/Module_/Crew_/Ammo_`
  role-prefixes (a second source of truth for role, now owned by RON) → all 44 volumes are
  `Ballistic_<part>`. Renamed across `.blend` (headless Blender), `.glb` (surgical JSON node-name
  patch — visual untouched, no re-export), the RON keys, and the bind's drift lint. The model's mesh
  kinds now split by *purpose* (ballistic / visual / collider / rig), not role. Bonus: the merged
  barrel mesh (Blender kept the name `Armor_Gun_Barrel`) became `Ballistic_Gun_Barrel` and the RON's
  `hp` makes it a module — name no longer matters. Binds clean, zero drift.
- **Model restructure cut (2026-06-28):** rig nodes (`Hull`, `Turret`, `Gun`, `Gun_Barrel`, 16
  `Wheel_*`) were meshes doing double duty as visual + pivot; converted to **Empties** (pivots) with
  their geometry demoted to `Visual_*` leaves — done headless via Blender (`mesh→empty` + transform-
  preserving re-parent), re-exported, verified (74 meshes / 3 materials / 3 images unchanged, modifiers
  0). Running gear → `Visual_*`; `Hull_Collider`→`Collider_Hull`, dormant `Turret_Collision`→
  `Collider_Turret` (now a proper convex proxy). Code: collider match → `Collider_` prefix. Three
  shape kinds (Visual / Ballistic / Collider) now split by purpose; rig is a pure empty skeleton. Binds
  clean. **Visual skin needs a human eyeball in the sandbox** (counts match, so geometry should be
  intact; backups in scratchpad).
- **Next:** consequences of HP→0 (§§7–8: ammo cookoff, crew death, module knock-out + degraded
  performance); persist material factors + HP to **RON keyed by node name** (§9 data home); then the
  deferred shape/tuning knobs (caliber exponent, fragment cap, `spall_factor` brittleness split).
