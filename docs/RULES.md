# Battle rules

Eiketsuden Reloaded reproduces the rules of the **PC version** of Sangokushi Eiketsuden (PC-98 / Korean DOS),
which is the version most players in Korea know. Formulas marked *(community)* were reverse-engineered by fans
(namu.wiki, Japanese handbook reviews, Daum cafe analyses); values marked *(design)* are our own choices where no
source exists. All numbers that a pack can change live in `rules/*.toml`; everything else here is engine behaviour.

Integer arithmetic: every division truncates towards zero unless stated otherwise. Percentages are applied as
`value * pct / 100`.

## 1. Turn structure

1. A turn has three phases: **player → ally → enemy**. A phase with no active units is skipped (no events fire for it
   except `turn_start` triggers).
2. **Phase start** (in this order, for the units of the side whose phase starts):
   1. On the player phase only: `turn += 1` (except the very first), then the weather is rolled (§8) and
      `WeatherChanged` is emitted when it differs from the previous turn.
   2. For every active unit of the side, in unit order: terrain regeneration (`heal_hp` % of max HP, `heal_morale`),
      equipment regeneration (`regen_hp`, `regen_morale`), military-band aura MP (§7.4). One `Regenerated` event per
      unit that gained anything.
   3. Status countdown (§6): each `Confused` status loses one turn; at 0 it is removed (`StatusExpired`) — unless the
      unit's morale is ≤ `confuse_morale`, in which case it stays at 1.
   4. Low-morale confusion (§6).
   5. `moved`/`acted` are cleared for the side.
   6. `turn_start` events for `(turn, side)` fire (§9).
3. Each unit of the phase may **move once, then perform one action** (attack, strategy, item, wait). Acting without
   moving is allowed. After the action the unit is done for the phase. A unit that moved but has not acted yet can
   still act; the frontend implements "cancel move" by restoring a snapshot.
4. `EndPhase` ends the phase; remaining units forfeit. After the enemy phase of turn `turn_limit` the battle is lost
   (`DefeatReason::TurnLimit`) unless a victory condition was met.
5. AI units act in unit-list order (spawn order); reinforcements act after the units that were on the map first.

## 2. Unit values

| Value | Formula |
|---|---|
| Max HP (troops) | `class.hp + class.hp_growth * (level - 1)` |
| Max MP | `min(mp_cap, (level + 10) * int / 40)` |
| stat term | `s(x) = 4000 / (140 - x)` in tenths (so `x = 100 → 100`, `x = 75 → 61`); `x` clamped to 0..=139 |
| ATK | `(level + 10) * (morale + s(str) + class.atk * 10) / 10`, then `* best weapon atk_pct / 100` (no weapon → ×1) |
| DEF | `(level + 10) * (morale + s(lead) + class.def * 10) / 10`, then `* best armor def_pct / 100` |
| Move | `class.move + best accessory move_bonus` (0 while confused) |

The ATK / DEF formula is the original's *(code: `MAIN.EXE` image 0x13A3E / 0x13B6C, FORMATS §10.4)*, which adds
twice a class coefficient stored as `class.atk * 5`. So is the max HP *(code: image 0x14308)*: the original stores
`class.hp / 100` and `class.hp_growth / 10`.

*(community)* Worked example: Cao Cao Lv42, STR 75, LEAD 98, guard class (atk 16 / def 12), morale 100, weapon 120%,
manual 120% → ATK `52 * (100 + 61 + 160) / 10 = 1669 → 2002`, DEF `52 * (100 + 95 + 120) / 10 = 1638 → 1965`.

Only one item per slot exists in this engine, so "best" is simply the equipped item. HP/MP are fully restored and
morale reset to `morale_start` at the start of every battle (the campaign keeps level/EXP/class/equipment only).

## 3. Movement

* Movement cost of a tile = `terrain.cost[class.move_type]`; a missing entry means the tile cannot be entered.
* Units cannot enter tiles occupied by **hostile** units. They may pass through friendly units (player ↔ ally are
  friends) but cannot end their move on an occupied tile.
* **Zone of control**: the 4 orthogonal neighbours of every active hostile unit. Entering a ZOC tile costs its normal
  cost but ends movement there (the tile is reachable, nothing beyond it through that path). Starting inside a ZOC does
  not restrict leaving it.
* The movement range is a Dijkstra search over those rules with `move_points`; ties prefer the path found first in
  the order up, down, left, right (deterministic). Out-of-map tiles are never reachable.
* Stepping on an untaken treasure tile ends the move; the treasure goes to the player (§10) — only player units take
  treasures.

## 4. Physical attacks

* Attack targets = hostile active units on tiles of `class.range` offsets from the attacker's tile.
* **Damage** *(community, deterministic — no miss, no critical, no double attack)*:

  ```
  def_eff = DEF * affinity_pct(attacker.family, defender.family) / 100
  damage  = max(1, (ATK - def_eff / 2) * (100 - terrain.defense) / 100)
  ```

  `affinity` (in `game.toml`) is 75 when the attacker's family has the advantage and 125 when it has the
  disadvantage (PC: infantry & bandit > archer > cavalry > infantry & bandit; other families neutral).
* The defender loses `ceil(damage * morale_loss_pct / max_hp)` morale (clamped at 0).
* **Counter-attack** *(community)*: happens only when **all** of these hold: the defender survived, its class has
  `can_counter`, the attacker's class has `provokes_counter`, and the attacker is orthogonally or diagonally adjacent
  (Chebyshev distance 1) and inside the defender's own attack range. Chance = `defender.str * 100 / counter_divisor`
  (PC: `str / 150`), rolled with the battle RNG. Counter damage = normal damage formula with roles swapped, times
  `counter_damage_pct / 100` *(design; the original is "weak", formula unknown)*, minimum 1. Counters give EXP and
  can defeat the attacker. Strategies never provoke counters.
* Events: one `Strike` for the attack, one more `Strike { counter: true }` for a counter.
* **Joint attack** (extended rules only, chosen for a new game; DECISIONS D25): the attack's damage (not a counter's)
  rises by 10 % for every other active unit of the attacker's side (players and allies are one side) orthogonally next
  to the defender, at most 30 %, rounded down: `damage * (100 + pct) / 100`. Where the attacker strikes from does not
  matter. Off by default.

## 5. Strategies

* A unit knows the strategies in the `strategies` lists of its class **and all classes before it in the promotion chain** with
  `level ≤ unit level`. Casting needs `mp ≥ strategy.mp`; confused units cannot cast.
* **Reach**: tiles of `strategy.range` offsets around the caster (`range8/12/20` include the caster's tile).
* **Area**: `single` = the unit on the aimed tile; `cross` = units on the aimed tile and its 4 neighbours;
  `all_in_range` = every valid target on the reach tiles (no aiming; the frontend aims at the caster).
* **Valid targets**: `target = enemy` → hostile units; `target = ally` → the caster's side and its friends (the caster
  included).
* **Element gate**: if `strategy.element` is set, each affected unit's tile must list the element in
  `terrain.elements`, and in rain `fire` is impossible. For `single`/`cross` the **aimed tile** must pass the gate for the
  strategy to be castable; units in the cross arms on non-matching tiles are skipped.
* **Hit chance** for enemy-targeted strategies *(community)*:

  ```
  power(u) = u.int * u.level / 100 + u.int        (defender's int doubled if its class has strategy_guard)
  chance % = clamp(100 - 100 * power(defender) / (4 * power(caster)), 0, 100)
  ```

  Heals and ally-targeted strategies always succeed. A missed target shows as a `StrategyHit { success: false }`.
* **Damage** (`Effect::Damage { power }`, *community*):

  ```
  raw = power + 2 * (caster.int * caster.level / 50 + caster.int) - (target.int * target.level / 50 + target.int)
  raw = raw * 125 / 100   if the element is boosted on the target tile (fire in forest) or it is water in rain
  raw = raw / 2           if the target's class has strategy_guard
  damage = max(1, raw) + rng.range(0, max(1, raw) / 50)
  ```

  Damage costs morale like a physical hit.
* **Heal** (`Effect::Heal { power }`, *design*): `power + 2 * (caster.int * caster.level / 50 + caster.int)`, halved when
  the target's morale is below 30; capped at the missing HP.
* **Morale** (`Effect::Morale { amount }`): positive amounts always apply; negative amounts (morale-down) need a hit
  and are shifted by `caster.level / 10 - target.level / 10` (a Lv5x caster against a Lv2x target: `-20 → -23`).
* **Status** (`Effect::Status { confused, turns }`): on hit, the unit becomes confused for `turns` turns.
* The caster pays the MP even if every target evades.
* **Original formulas** (`strategy_formulas = "original"` in `rules/game.toml`, the PC original's routines in
  `MAIN.EXE`, FORMATS §10.4 *[code]*; the original mode's pack uses them). Everything above holds except:
  * the hit chance of a strategy with a confusion effect divides by `2 * power(caster)` instead of
    `4 * power(caster)` (for all its effects; the original has no strategy that also damages);
  * damage has no minimum: `max(0, raw)`;
  * the random bonus of damage is `r` with `0 ≤ r < raw / 50` (the original's `rand`), not `0..=raw / 50`;
  * heal = `power + caster.level * caster.int / 20` (no halving on low morale), plus `r` with `0 ≤ r < heal / 10`
    when cast (the forecast shows it without);
  * a morale gain adds `caster.level / 10`, plus `r` with `0 ≤ r < gain / 10` when cast;
  * confusion has no length (the HUD shows no count): it ends at the start of the unit's side's phase with chance
    `(lead + morale) / 3` percent (§6), and morale that a morale-down, strategy or physical damage or a counter
    takes below 30 confuses the unit with 60 % (the original script's side-wide halving is not converted: BACKLOG).

## 6. Morale and confusion

* Morale starts at `morale_start` (100) each battle, drops when taking damage (§4, §5), and is raised by strategies,
  items, and terrain/equipment regeneration. Range 0..=100. Morale enters ATK and DEF directly (§2).
* **Low-morale confusion** *(design; the original confuses units "around 30" morale)*: at the start of its side's
  phase an unconfused unit with `morale ≤ confuse_morale` becomes confused for 1 turn with chance
  `(confuse_morale - morale) * 3 + 10` percent.
* Under the original strategy formulas (§5), as the original's morale setter and phase loops do (FORMATS §8, §10.4
  *[code]*):
  * there is no low-morale confusion at a phase start (`confuse_morale` is not used); instead, whenever a unit's
    morale falls and ends below 30 (damage, counters, morale-down strategies) it becomes confused with 60 %;
  * a confusion has no length: at the start of its side's phase a confused unit recovers with chance
    `(lead + morale) / 3` percent, and the same roll is made whenever its morale rises (regeneration — before the
    phase-start roll —, support strategies, items; also a morale item or regeneration on a unit already at 100, or a blow that costs it no morale there);
  * a unit defeated by the blow is not confused, and a confused defender (also one the blow just confused) does not
    counter.
* A confused unit cannot move or act; the AI skips it; the player cannot select it for commands.
* **A confused unit whose morale reaches 0 retreats immediately** (no defeat EXP is awarded to anyone).

## 7. Experience, levels, classes

1. EXP sources (level difference `d = target.level - own.level`):
   * physical attack or attack strategy that damages: `exp_attack[d]` per damaged unit;
   * defeating a unit: `exp_kill[d]` instead of `exp_attack[d]` for that unit, plus `exp_commander` if it was a commander;
   * successful heal/morale/confusion strategy: `class.support_exp` (single-target) or `exp_support`; once per cast;
   * counter-attacks earn EXP like attacks; items earn none, except strategy scrolls which earn like the strategy;
   * the battle bonus objective gives `bonus.exp` to every surviving deployed player unit at victory.
2. At `exp ≥ exp_per_level` the unit levels up (repeat while enough EXP), keeping the remainder; nothing happens at
   `level_cap` (EXP stays at `exp_per_level - 1`). Level up raises max HP by `hp_growth` and current HP by the same
   amount, recomputes max MP (current MP rises by the difference), and emits `LevelUp` plus `Learned` for each
   newly known strategy.
3. **Promotion** (PC): a class with `promote` changes to `promote.to` when the class-up item `promote.item` is used on an
   officer of at least `promote.level` (in camp: `CampaignState::use_item`). Level is kept; strategies of the old class
   stay known.
4. **Class change items** (`Effect::ChangeClass`): switch to the given class, keeping level; impossible for officers with
   `fixed_class`.
5. **Military band aura**: at the start of every phase, each active unit orthogonally adjacent to an active unit
   whose class has `mp_aura` regains `band.level / 10 + 1` MP per adjacent band (any side).
6. **Difficulty** (DECISIONS D25): chosen for a new game and kept in the save. When a battle is set up, every enemy
   unit's level (named officers and generic units, reinforcements included) moves by the difficulty's offset — easy −2,
   normal 0, hard +2 — at least 1, and hard never raises it past `level_cap`; a pack level already above the cap is
   not lowered to it (easy still takes 2 off). Player units,
   allies and guests are unchanged. Everything that depends on the level follows (§2, learned strategies, the EXP
   level difference above). Normal is the pack as it is.

## 8. Weather

Rolled at the start of every player phase from `game.toml` `weather` chances: clear, cloudy, rain. Rain makes fire
strategies impossible and water strategies deal +25%. Battles may override the chances in future versions.

## 9. Battle events, conditions, outcome

* Unit references in conditions/events (`target`, `who`, `a`, `b`) match a unit's `tag` or officer id.
* Events are checked after every action, after every phase start and after spawns; each fires at most once when
  `once = true` (default). Actions run in order. A `drama` action emits `BattleEvent::Drama` for the frontend.
  `spawn` places every hidden unit of the group; if its tile is occupied or impassable for it, the nearest free
  passable tile (by manhattan distance, then row-major) is used. A `retreat` action on a hidden unit takes it out
  of the battle (`Retreated`) without showing anything: a later `spawn` of its group does not bring it in.
* **Victory** when any `victory` condition holds; **defeat** when any `defeat` condition holds, the lord retreats, or
  the turn limit passes. A battle fought without the lord (the lord in `deploy.forbidden`: another troop's battle) is
  also lost when every player unit on the map has retreated. Checked after every action and phase change; victory is
  checked before defeat, except that the lord retreating always loses, before the events run. A lordless troop
  that has retreated (none of its player units left, hidden reinforcements included) loses after the events and the
  victory check. On victory: `reward_gold` is added to `gold_found`, bonus EXP is granted,
  `Victory` is emitted.
* `defeat_all` counts only enemies that are on the map (hidden reinforcements do not count).
* The bonus objective is checked like a victory condition; when it becomes true `bonus_done` is set and
  `BonusAchieved` emitted (EXP is granted at victory).

## 10. Items in battle

* Battle consumables (`battle_use = true`) come from the army inventory. A unit uses one as its action on itself or an
  orthogonally adjacent friendly unit (`effects`: `heal` restores exactly `power` HP, `morale` adds morale; under the
  original strategy formulas (§5) plus `r` with `0 ≤ r < value / 10` each, as the original heals through the
  support strategies' routine without a caster; the forecast shows it without), or — for items with `strategy` — casts that strategy from its tile without
  paying MP.
* A battle uses its own stock of consumables, copied from the army inventory when it starts. When it ends, won or
  lost, exactly the consumables it used (`items_used`) are taken out of the inventory (never below 0). An item a
  scene gives while the battle runs (`@item` in the intro or a `drama` event) reaches the inventory but cannot be
  used in that battle — see [MODDING.md, Treasures](MODDING.md#treasures).
* Treasure tiles give their item (to `items_found`) and/or gold (to `gold_found`) to the first player unit that ends a
  move there. Units with `drop` give that item to the player when defeated. Found items and gold only reach the
  campaign after a victory.

## 11. Retreat

At 0 HP a unit retreats (`Retreated`) and leaves the map; it is not dead and fights again in the next battle.
The lord retreating loses the battle immediately; so does, in a battle without the lord, the last player unit on
the map retreating.

## 12. AI

* AI units (enemy, ally, and the player side in simulations) choose among all reachable tiles × actions, scoring:
  damage dealt (×3 if it defeats the target, +50% against the lord or a commander), healing on hurt friends,
  confusion/morale-down on dangerous targets, minus the expected damage the unit would receive on that tile from
  hostile units next phase, plus terrain defence. Strategies are used when they out-score the best physical attack.
* Mode rules: `hold` never moves; `defensive` stays until a hostile unit can be reached this phase; `guard` stays within
  3 tiles of `ai_pos` (default: spawn tile); `target` moves towards `ai_target`; `advance` moves towards `ai_pos` (entering it once in reach) and, within 3 tiles of it, holds it like a `guard` post: it acts from `ai_pos` when it can, else from a tile within those 3 tiles, else returns to it;
  `flee` maximises distance from hostile units; `march` heads for `ai_target` or `ai_pos` without acting (and is no
  threat to plan around); `aggressive` approaches the best target anywhere.
* When no action is possible, the unit moves towards its goal (nearest hostile unit, target or position) along the
  cheapest path and waits.
* The player side in simulations and lords play carefully *(design)*: they keep off tiles where they would be
  defeated and act only when that beats moving on. When one of them has nothing better to do than an idle move
  (`aggressive`, which the player side plays), is below 50% HP and cannot get closer to its goal, it goes onto a
  healing tile (`heal_hp`) it can safely stand on, or towards the nearest free one it can walk to; any careful
  unit on a healing tile (even one it only passed by) stays there until it is back at 75%.
* The AI is deterministic for a given state (ties broken by unit id, then position order).

## 13. The forbidden secret (hidden command)

The PC original hides a command behind the lord's portrait; the game keeps it (`crate::secret` in
`hero-game`, `CampaignState::forbidden_secret` in `hero-core`). Read from the Korean `MAIN.EXE` (the
counter at DS `0x2D54`, the handler at image `0x1D3F2`, the orb effects at `0x1FD14`):

* On a non-battle screen, tap the lord's portrait. Here: 무장 정보 in the camp, on the lord's detail page or,
  in a pack with the original's status window (the original mode), on that window's portrait while the lord
  is the chosen officer.
  Like the original, it takes the mouse (or touch): keys do not count.
* The 44th tap plays a chime and arms the prompt; the 9th tap after that asks whether to use it. "No"
  disarms it but keeps the count, so the chime comes again only when the count reaches 44 once more.
  Nothing is saved: a new game or a loaded save starts over.
* "Yes" shows a small blue orb in the top left corner. Tapping it gives the lord the level cap
  (`rules.level_cap`, 99 in the base pack) with no spare EXP, 100 in 무력, 지력 and 통솔, and 10000 gold
  (clamped to `gold_cap`). It can be tapped again. Once enabled, taps on the portrait page through the
  army again.
* The original has four more orbs (every other officer to level 1, a sound test, item values 255, one
  without effect); they are not implemented.
