//! The generic crafter: a machine that runs one recipe of its crafting categories.
//!
//! - The machine has one input buffer for each recipe input and one output buffer for each
//!   recipe output and byproduct. Each buffer holds `buffer_crafts` crafts (default 2).
//! - A craft starts when all inputs are there and all outputs have room for one more craft.
//!   Starting a craft uses the inputs.
//! - Work per tick is `speed × 2^(tiers above the recipe tier) × power factor`, in ticks of
//!   recipe time. A craft of `time` seconds needs `time × 60` of work (game design section 12.2).
//! - Byproducts use a random number from `foundry_core::Rng` with a seed made from the building
//!   and the number of finished crafts. So the result is the same in every run.
//! - A burner machine (a campfire: a crafter with the params `fuel_capacity` and `fuel_ticks`)
//!   has a fuel slot (`Fuel`). Each unit of fuel gives `fuel_ticks` ticks of work. The machine
//!   burns fuel only while a craft runs. With no fuel it stops ("No fuel").
//!
//! Ports and the world are handled in `buildings.rs`. This file has no world access.

use foundry_content::{Content, ItemRef, Recipe, Stack};
use foundry_core::{MaterialId, RecipeId, Rng, TICKS_PER_SECOND};
use serde::{Deserialize, Serialize};

/// What a building is doing. The building window shows it with a reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Status {
    /// Nothing to do, and nothing is wrong (storage, walls, an empty belt).
    #[default]
    Idle,
    Working,
    /// A machine without a recipe.
    NoRecipe,
    /// Waiting for input.
    NoInput,
    /// The output buffer is full and no port can take the output.
    OutputFull,
    /// An output port or exhaust has no free cells in front of it.
    OutputBlocked,
    NoPower,
    /// A burner machine has no fuel in its fuel slot.
    NoFuel,
    /// The recipe needs a higher temperature.
    TooCold,
    /// The body is hotter than the building's maximum temperature.
    TooHot,
    /// Hit points are at 0.
    Broken,
    /// A room machine controller whose room is not closed or not correct (see `rooms`).
    NoRoom,
}

impl Status {
    /// A short text for the player.
    pub const fn text(self) -> &'static str {
        match self {
            Status::Idle => "Idle",
            Status::Working => "Working",
            Status::NoRecipe => "No recipe",
            Status::NoInput => "No input",
            Status::OutputFull => "Output full",
            Status::OutputBlocked => "Output blocked",
            Status::NoPower => "No power",
            Status::NoFuel => "No fuel",
            Status::TooCold => "Too cold",
            Status::TooHot => "Too hot",
            Status::Broken => "Broken",
            Status::NoRoom => "Room not valid",
        }
    }

    /// True if the building does not do its work because of a problem the player should fix.
    pub const fn is_problem(self) -> bool {
        !matches!(self, Status::Idle | Status::Working)
    }
}

/// Numbers from outside the machine for one tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Conditions {
    /// Work per tick, in ticks of recipe time.
    pub speed: f64,
    pub has_power: bool,
    /// The body is too hot to work.
    pub too_hot: bool,
    /// The temperature that `min_temp` is checked against (°C).
    pub heat: i16,
}

/// The state of one crafter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub recipe: Option<RecipeId>,
    /// The count in each input buffer, in the order of `recipe.inputs`.
    pub inputs: Vec<u32>,
    /// The count in each output buffer: `recipe.outputs`, then `recipe.byproducts`.
    pub outputs: Vec<u32>,
    /// Work done on the current craft, in ticks of recipe time.
    pub work: f64,
    /// A craft is running: its inputs are used.
    pub running: bool,
    /// Crafts finished since the machine was built.
    pub crafts: u64,
    /// Each buffer holds this many crafts.
    pub buffer_crafts: u32,
    /// The fuel slot of a burner machine. `None`: the machine needs no fuel.
    #[serde(default)]
    pub fuel: Option<Fuel>,
}

/// The fuel slot of a burner machine (a campfire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Fuel {
    /// The fuel material in the slot. `None` when the slot is empty.
    pub material: Option<MaterialId>,
    pub units: u32,
    /// The most units the slot holds.
    pub capacity: u32,
    /// Ticks of work that one unit of fuel gives.
    pub ticks_per_unit: u32,
    /// Ticks of work left from the unit that burns now.
    pub burn: u32,
}

impl Fuel {
    pub fn new(capacity: u32, ticks_per_unit: u32) -> Self {
        Self { material: None, units: 0, capacity, ticks_per_unit: ticks_per_unit.max(1), burn: 0 }
    }

    /// How many units of `m` the slot can take now. The slot holds one material at a time.
    pub fn room(&self, m: MaterialId) -> u32 {
        match self.material {
            Some(x) if x != m && self.units > 0 => 0,
            _ => self.capacity.saturating_sub(self.units),
        }
    }

    /// Put up to `n` units of `m` into the slot. Returns the count taken.
    pub fn add(&mut self, m: MaterialId, n: u32) -> u32 {
        let n = n.min(self.room(m));
        if n > 0 {
            self.material = Some(m);
            self.units += n;
        }
        n
    }

    /// Take up to `n` units out of the slot. Returns the material and the count taken.
    pub fn take(&mut self, n: u32) -> Option<(MaterialId, u32)> {
        let m = self.material?;
        let t = self.units.min(n);
        self.units -= t;
        if self.units == 0 {
            self.material = None;
        }
        (t > 0).then_some((m, t))
    }

    /// Make sure a unit burns for this tick. False if there is no fuel.
    fn light(&mut self) -> bool {
        if self.burn > 0 {
            return true;
        }
        if self.take(1).is_none() {
            return false;
        }
        self.burn = self.ticks_per_unit;
        true
    }
}

/// Why a recipe cannot be set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipeError {
    NotAMachine,
    /// The machine does not make this category.
    WrongCategory { recipe: String, building: String },
    /// The recipe tier is above the machine tier.
    TierTooLow { needs: u8, has: u8 },
    /// The player has not researched the recipe yet.
    NotKnown { recipe: String },
}

impl std::fmt::Display for RecipeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecipeError::NotAMachine => write!(f, "This building has no recipes"),
            RecipeError::WrongCategory { recipe, building } => write!(f, "{building} cannot make {recipe}"),
            RecipeError::TierTooLow { needs, has } => write!(f, "Needs a tier {needs} machine; this one is tier {has}"),
            RecipeError::NotKnown { recipe } => write!(f, "Research {recipe} first"),
        }
    }
}

impl std::error::Error for RecipeError {}

/// True if a burner machine burns this material: it has the tag `fuel` and it is a powder or a
/// solid (a campfire does not burn oil).
pub fn is_fuel(content: &Content, m: MaterialId) -> bool {
    let Some(bit) = content.tags.bit("fuel") else { return false };
    content.materials.has_tag(m, bit)
        && matches!(content.materials.phase[m.index()], foundry_content::Phase::Powder | foundry_content::Phase::Solid)
}

/// Tiers of the machine above the recipe tier.
pub fn overclock(machine_tier: u8, recipe_tier: u8) -> u32 {
    machine_tier.saturating_sub(recipe_tier) as u32
}

/// Speed factor for tier overclocking: 2 times faster for each tier above the recipe.
pub fn overclock_speed(machine_tier: u8, recipe_tier: u8) -> f64 {
    (1u64 << overclock(machine_tier, recipe_tier).min(16)) as f64
}

/// Power factor for tier overclocking: 4 times the power for each tier above the recipe.
pub fn overclock_power(machine_tier: u8, recipe_tier: u8) -> f64 {
    (1u64 << (2 * overclock(machine_tier, recipe_tier).min(16))) as f64
}

/// The item and count of output buffer `j` (outputs, then byproducts).
pub fn output_stack(recipe: &Recipe, j: usize) -> Stack {
    if j < recipe.outputs.len() { recipe.outputs[j] } else { recipe.byproducts[j - recipe.outputs.len()].0 }
}

/// Ticks of work for one craft.
pub fn work_needed(recipe: &Recipe) -> f64 {
    (recipe.time as f64 * TICKS_PER_SECOND as f64).max(1.0)
}

impl Machine {
    pub fn new(buffer_crafts: u32) -> Self {
        Self {
            recipe: None,
            inputs: vec![],
            outputs: vec![],
            work: 0.0,
            running: false,
            crafts: 0,
            buffer_crafts: buffer_crafts.max(1),
            fuel: None,
        }
    }

    /// A burner machine: a machine with a fuel slot.
    pub fn with_fuel(mut self, fuel: Fuel) -> Self {
        self.fuel = Some(fuel);
        self
    }

    /// Capacity of input buffer `k`.
    pub fn input_capacity(&self, recipe: &Recipe, k: usize) -> u32 {
        recipe.inputs[k].count.saturating_mul(self.buffer_crafts)
    }

    /// Capacity of output buffer `j`.
    pub fn output_capacity(&self, recipe: &Recipe, j: usize) -> u32 {
        output_stack(recipe, j).count.saturating_mul(self.buffer_crafts)
    }

    /// How many of `item` the input buffers can take now.
    pub fn input_room(&self, recipe: &Recipe, item: ItemRef) -> u32 {
        recipe
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, s)| s.item == item)
            .map(|(k, _)| self.input_capacity(recipe, k) - self.inputs[k].min(self.input_capacity(recipe, k)))
            .sum()
    }

    /// Put up to `n` of `item` into the input buffers. Returns the count taken.
    pub fn add_input(&mut self, recipe: &Recipe, item: ItemRef, n: u32) -> u32 {
        let mut left = n;
        for k in 0..recipe.inputs.len() {
            if recipe.inputs[k].item != item || left == 0 {
                continue;
            }
            let room = self.input_capacity(recipe, k).saturating_sub(self.inputs[k]);
            let add = room.min(left);
            self.inputs[k] += add;
            left -= add;
        }
        n - left
    }

    /// Take up to `n` from output buffer `j`. Returns the count taken.
    pub fn take_output(&mut self, j: usize, n: u32) -> u32 {
        let Some(have) = self.outputs.get_mut(j) else { return 0 };
        let t = (*have).min(n);
        *have -= t;
        t
    }

    /// Set a new recipe (or none). Returns everything that was in the buffers, including the inputs
    /// of a running craft.
    pub fn set_recipe(&mut self, content: &Content, recipe: Option<RecipeId>) -> Vec<Stack> {
        let old = self.take_contents(content);
        self.recipe = recipe;
        match recipe {
            Some(r) => {
                let def = content.factory.recipe_def(r);
                self.inputs = vec![0; def.inputs.len()];
                self.outputs = vec![0; def.outputs.len() + def.byproducts.len()];
            }
            None => {
                self.inputs.clear();
                self.outputs.clear();
            }
        }
        old
    }

    /// Empty all buffers. A running craft is stopped and its inputs are given back.
    pub fn take_contents(&mut self, content: &Content) -> Vec<Stack> {
        let mut out: Vec<Stack> = vec![];
        let Some(r) = self.recipe else { return out };
        let recipe = content.factory.recipe_def(r);
        let mut add = |s: Stack| {
            if s.count == 0 {
                return;
            }
            match out.iter_mut().find(|o| o.item == s.item) {
                Some(o) => o.count += s.count,
                None => out.push(s),
            }
        };
        for (k, n) in self.inputs.iter_mut().enumerate() {
            let mut count = *n;
            if self.running {
                count += recipe.inputs[k].count;
            }
            add(Stack { item: recipe.inputs[k].item, count });
            *n = 0;
        }
        for (j, n) in self.outputs.iter_mut().enumerate() {
            add(Stack { item: output_stack(recipe, j).item, count: *n });
            *n = 0;
        }
        self.running = false;
        self.work = 0.0;
        out
    }

    /// Empty the fuel slot. For a removed building (a new recipe keeps the fuel).
    pub fn take_fuel(&mut self) -> Option<Stack> {
        let f = self.fuel.as_mut()?;
        let (m, n) = f.take(u32::MAX)?;
        Some(Stack { item: ItemRef::Material(m), count: n })
    }

    /// Why a new craft cannot start, or `None` if it can.
    fn start_problem(&self, recipe: &Recipe) -> Option<Status> {
        if recipe.inputs.iter().enumerate().any(|(k, s)| self.inputs[k] < s.count) {
            return Some(Status::NoInput);
        }
        let full = (0..self.outputs.len()).any(|j| self.outputs[j] + output_stack(recipe, j).count > self.output_capacity(recipe, j));
        full.then_some(Status::OutputFull)
    }

    /// The first output buffer that has no room for another craft.
    pub fn full_output(&self, recipe: &Recipe) -> Option<usize> {
        (0..self.outputs.len()).find(|&j| self.outputs[j] + output_stack(recipe, j).count > self.output_capacity(recipe, j))
    }

    /// The first input that is missing for the next craft, with the count still needed.
    pub fn missing_input(&self, recipe: &Recipe) -> Option<Stack> {
        recipe
            .inputs
            .iter()
            .enumerate()
            .find(|(k, s)| self.inputs[*k] < s.count)
            .map(|(k, s)| Stack { item: s.item, count: s.count - self.inputs[k] })
    }

    fn start(&mut self, recipe: &Recipe) {
        for (k, s) in recipe.inputs.iter().enumerate() {
            self.inputs[k] -= s.count;
        }
        self.running = true;
    }

    fn finish(&mut self, recipe: &Recipe, seed: u64) {
        for (j, s) in recipe.outputs.iter().enumerate() {
            self.outputs[j] += s.count;
        }
        let mut rng = Rng::new(seed ^ self.crafts.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        for (i, (s, chance)) in recipe.byproducts.iter().enumerate() {
            if rng.chance(*chance) {
                self.outputs[recipe.outputs.len() + i] += s.count;
            }
        }
        self.crafts += 1;
        self.running = false;
    }

    /// Run one tick. `seed` is a fixed number for this building (for byproduct chances).
    pub fn step(&mut self, recipe: &Recipe, c: &Conditions, seed: u64) -> Status {
        if !self.running
            && let Some(problem) = self.start_problem(recipe)
        {
            self.work = 0.0;
            return problem;
        }
        if c.too_hot {
            return Status::TooHot;
        }
        if !c.has_power {
            return Status::NoPower;
        }
        if let Some(t) = recipe.min_temp
            && c.heat < t
        {
            return Status::TooCold;
        }
        if let Some(f) = &mut self.fuel {
            if !f.light() {
                return Status::NoFuel;
            }
            f.burn -= 1;
        }
        if !self.running {
            self.start(recipe);
        }
        let need = work_needed(recipe);
        self.work += c.speed;
        // A fast machine can finish more than one craft in a tick.
        for _ in 0..64 {
            if self.work < need {
                break;
            }
            self.finish(recipe, seed);
            self.work -= need;
            if self.start_problem(recipe).is_some() {
                self.work = 0.0;
                break;
            }
            self.start(recipe);
        }
        Status::Working
    }

    /// Progress of the running craft, 0 to 1.
    pub fn progress(&self, recipe: &Recipe) -> f32 {
        if !self.running {
            return 0.0;
        }
        (self.work / work_needed(recipe)).clamp(0.0, 1.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn content() -> Arc<Content> {
        Arc::new(Content::load_default().expect("content loads"))
    }

    fn cond(speed: f64) -> Conditions {
        Conditions { speed, has_power: true, too_hot: false, heat: 20 }
    }

    #[test]
    fn a_craft_takes_time_times_60_ticks() {
        let c = content();
        let r = c.factory.recipe("bronze_gear").unwrap(); // 3 bronze plates -> 1 gear, 2 s
        let recipe = c.factory.recipe_def(r);
        let mut m = Machine::new(2);
        m.set_recipe(&c, Some(r));
        let plate = c.item("bronze_plate").unwrap();
        assert_eq!(m.input_room(recipe, plate), 6);
        assert_eq!(m.add_input(recipe, plate, 10), 6);
        for _ in 0..119 {
            assert_eq!(m.step(recipe, &cond(1.0), 1), Status::Working);
        }
        assert_eq!(m.outputs[0], 0);
        assert_eq!(m.step(recipe, &cond(1.0), 1), Status::Working);
        assert_eq!(m.outputs[0], 1);
        // The second craft started at once.
        assert!(m.running);
        assert_eq!(m.inputs[0], 0);
        for _ in 0..120 {
            m.step(recipe, &cond(1.0), 1);
        }
        assert_eq!(m.outputs[0], 2);
        assert_eq!(m.step(recipe, &cond(1.0), 1), Status::NoInput);
    }

    #[test]
    fn full_output_stops_the_machine_and_contents_come_back() {
        let c = content();
        let r = c.factory.recipe("bronze_gear").unwrap();
        let recipe = c.factory.recipe_def(r);
        let plate = c.item("bronze_plate").unwrap();
        let mut m = Machine::new(1);
        m.set_recipe(&c, Some(r));
        m.add_input(recipe, plate, 3);
        for _ in 0..120 {
            m.step(recipe, &cond(1.0), 1);
        }
        assert_eq!(m.outputs[0], 1);
        m.add_input(recipe, plate, 3);
        assert_eq!(m.step(recipe, &cond(1.0), 1), Status::OutputFull);
        assert_eq!(m.full_output(recipe), Some(0));
        let back = m.set_recipe(&c, None);
        assert_eq!(back, vec![Stack { item: plate, count: 3 }, Stack { item: c.item("bronze_gear").unwrap(), count: 1 }]);
    }

    #[test]
    fn too_cold_does_not_start_and_running_inputs_are_returned() {
        let c = content();
        let r = c.factory.recipe("bronze_alloy").unwrap(); // needs 1000 °C
        let recipe = c.factory.recipe_def(r);
        let mut m = Machine::new(2);
        m.set_recipe(&c, Some(r));
        let cu = c.item("molten_copper").unwrap();
        let sn = c.item("molten_tin").unwrap();
        m.add_input(recipe, cu, 48);
        m.add_input(recipe, sn, 16);
        let mut cold = cond(1.0);
        cold.heat = 900;
        assert_eq!(m.step(recipe, &cold, 1), Status::TooCold);
        assert!(!m.running);
        let mut hot = cold;
        hot.heat = 1100;
        assert_eq!(m.step(recipe, &hot, 1), Status::Working);
        assert!(m.running);
        assert_eq!(m.inputs, vec![0, 0]);
        let back = m.take_contents(&c);
        assert_eq!(back, vec![Stack { item: cu, count: 48 }, Stack { item: sn, count: 16 }]);
    }

    #[test]
    fn overclock_numbers() {
        assert_eq!(overclock_speed(0, 0), 1.0);
        assert_eq!(overclock_speed(2, 0), 4.0);
        assert_eq!(overclock_power(2, 0), 16.0);
        assert_eq!(overclock_speed(0, 3), 1.0);
    }
}
