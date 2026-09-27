//! Factory content after loading: parts, buildings, recipes, technologies and milestones,
//! with number ids and lookups. See `factory_defs` for the file format.

use crate::factory_defs::*;
use crate::table::MaterialTable;
use foundry_core::{BuildingKindId, MaterialId, PartId, RecipeId, TechId};
use std::collections::{BTreeMap, HashMap};

/// An item: bulk material (counted in units = cells) or a part (counted in pieces).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemRef {
    Material(MaterialId),
    Part(PartId),
}

/// An amount of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stack {
    pub item: ItemRef,
    pub count: u32,
}

/// A port after loading. Filters are items.
#[derive(Debug, Clone, PartialEq)]
pub struct Port {
    pub kind: PortKind,
    pub tile: (u8, u8),
    pub side: Side,
    pub filter: Vec<ItemRef>,
    pub name: Option<String>,
}

/// A building type after loading.
#[derive(Debug, Clone, PartialEq)]
pub struct Building {
    pub id: String,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub size: (u8, u8),
    pub layer: Layer,
    pub tier: u8,
    pub body: MaterialId,
    pub hit_points: u32,
    pub max_temp: i16,
    pub ports: Vec<Port>,
    pub power: Option<PowerDef>,
    pub crafts: Vec<String>,
    pub speed: f32,
    pub params: BTreeMap<String, f32>,
    /// The item that places this building.
    pub part: PartId,
}

impl Building {
    /// A kind-specific number, or `default` if the data does not set it.
    pub fn param(&self, name: &str, default: f32) -> f32 {
        self.params.get(name).copied().unwrap_or(default)
    }
}

/// A part after loading.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub stack: u16,
    pub material: Option<MaterialId>,
    pub units: u16,
    pub icon: Option<String>,
    pub tags: Vec<String>,
    /// Set if this part places a building.
    pub building: Option<BuildingKindId>,
}

/// A recipe after loading.
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub category: String,
    pub hand: bool,
    pub inputs: Vec<Stack>,
    pub outputs: Vec<Stack>,
    /// (item, chance 0 to 1)
    pub byproducts: Vec<(Stack, f32)>,
    pub time: f32,
    pub tier: u8,
    pub min_temp: Option<i16>,
    /// Crafting menu tab.
    pub group: String,
    /// The technology that unlocks it. None: known from the start.
    pub unlocked_by: Option<TechId>,
}

/// A technology after loading.
#[derive(Debug, Clone, PartialEq)]
pub struct Tech {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tier: u8,
    pub requires: Vec<TechId>,
    pub kits: Vec<Stack>,
    pub units: u32,
    pub unit_time: f32,
    pub discoveries: Vec<String>,
    pub discovery_points: u32,
    pub unlocks: Vec<RecipeId>,
    pub effects: Vec<(String, f32)>,
}

/// A Hub repair stage after loading.
#[derive(Debug, Clone, PartialEq)]
pub struct Milestone {
    pub stage: u8,
    pub name: String,
    pub description: String,
    pub deliver: Vec<Stack>,
    pub unlocks_tier: u8,
}

/// All factory content. `parts[i]` has `PartId(i)`, `buildings[i]` has `BuildingKindId(i)`,
/// `recipes[i]` has `RecipeId(i)`, `techs[i]` has `TechId(i)`.
#[derive(Debug, Clone, Default)]
pub struct FactoryContent {
    pub parts: Vec<Part>,
    pub buildings: Vec<Building>,
    pub recipes: Vec<Recipe>,
    pub techs: Vec<Tech>,
    /// Sorted by stage.
    pub milestones: Vec<Milestone>,
    part_ids: HashMap<String, PartId>,
    building_ids: HashMap<String, BuildingKindId>,
    recipe_ids: HashMap<String, RecipeId>,
    tech_ids: HashMap<String, TechId>,
}

impl FactoryContent {
    pub fn part(&self, id: &str) -> Option<PartId> {
        self.part_ids.get(id).copied()
    }

    pub fn building(&self, id: &str) -> Option<BuildingKindId> {
        self.building_ids.get(id).copied()
    }

    pub fn recipe(&self, id: &str) -> Option<RecipeId> {
        self.recipe_ids.get(id).copied()
    }

    pub fn tech(&self, id: &str) -> Option<TechId> {
        self.tech_ids.get(id).copied()
    }

    pub fn part_def(&self, id: PartId) -> &Part {
        &self.parts[id.0 as usize]
    }

    pub fn building_def(&self, id: BuildingKindId) -> &Building {
        &self.buildings[id.0 as usize]
    }

    pub fn recipe_def(&self, id: RecipeId) -> &Recipe {
        &self.recipes[id.0 as usize]
    }

    pub fn tech_def(&self, id: TechId) -> &Tech {
        &self.techs[id.0 as usize]
    }

    /// Recipes a building crafting category can make.
    pub fn recipes_in_category<'a>(&'a self, category: &'a str) -> impl Iterator<Item = RecipeId> + 'a {
        self.recipes.iter().enumerate().filter(move |(_, r)| r.category == category).map(|(i, _)| RecipeId(i as u16))
    }

    /// Recipes that make an item ("how to make it").
    pub fn recipes_making(&self, item: ItemRef) -> impl Iterator<Item = RecipeId> + '_ {
        self.recipes
            .iter()
            .enumerate()
            .filter(move |(_, r)| r.outputs.iter().chain(r.byproducts.iter().map(|(s, _)| s)).any(|s| s.item == item))
            .map(|(i, _)| RecipeId(i as u16))
    }

    /// Recipes that use an item ("what uses it").
    pub fn recipes_using(&self, item: ItemRef) -> impl Iterator<Item = RecipeId> + '_ {
        self.recipes
            .iter()
            .enumerate()
            .filter(move |(_, r)| r.inputs.iter().any(|s| s.item == item))
            .map(|(i, _)| RecipeId(i as u16))
    }

    /// Build the factory content and check it. Errors are added to `errors`.
    pub fn build(
        mats: &MaterialTable,
        parts: Vec<PartDef>,
        buildings: Vec<BuildingDef>,
        recipes: Vec<RecipeDef>,
        techs: Vec<TechDef>,
        milestones: Vec<MilestoneDef>,
        errors: &mut Vec<String>,
    ) -> FactoryContent {
        let mut fc = FactoryContent::default();
        let id_ok = |id: &str| !id.is_empty() && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');

        // Parts: the listed parts, then one part for each building.
        let add_part = |fc: &mut FactoryContent, p: Part, errors: &mut Vec<String>| {
            if !id_ok(&p.id) {
                errors.push(format!("part id `{}`: use only a-z, 0-9 and _", p.id));
            }
            if mats.find(&p.id).is_some() {
                errors.push(format!("`{}` is both a material and a part", p.id));
            }
            if p.stack == 0 {
                errors.push(format!("part `{}`: stack must be at least 1", p.id));
            }
            let id = PartId(fc.parts.len() as u16);
            if fc.part_ids.insert(p.id.clone(), id).is_some() {
                errors.push(format!("part id `{}` is used twice", p.id));
            }
            fc.parts.push(p);
        };
        for d in parts {
            let material = d.material.as_deref().and_then(|m| {
                let r = mats.find(m);
                if r.is_none() {
                    errors.push(format!("part `{}` is made of `{m}`, which is not a material", d.id));
                }
                r
            });
            let p = Part {
                id: d.id,
                name: d.name,
                description: d.description,
                category: d.category,
                stack: d.stack,
                material,
                units: d.units,
                icon: d.icon,
                tags: d.tags,
                building: None,
            };
            add_part(&mut fc, p, errors);
        }
        for (i, d) in buildings.iter().enumerate() {
            let p = Part {
                id: d.id.clone(),
                name: d.name.clone(),
                description: d.description.clone(),
                category: d.category.clone().unwrap_or_else(|| "production".into()),
                stack: d.stack,
                material: mats.find(&d.body),
                units: 0,
                icon: d.icon.clone().or_else(|| Some("machine".into())),
                tags: d.tags.clone(),
                building: Some(BuildingKindId(i as u16)),
            };
            add_part(&mut fc, p, errors);
        }

        let item = |fc: &FactoryContent, id: &str, owner: &str, errors: &mut Vec<String>| -> Option<ItemRef> {
            if let Some(m) = mats.find(id) {
                return Some(ItemRef::Material(m));
            }
            if let Some(p) = fc.part_ids.get(id) {
                return Some(ItemRef::Part(*p));
            }
            errors.push(format!("{owner}: `{id}` is not a material or a part"));
            None
        };
        let stacks = |fc: &FactoryContent, list: &[(String, u32)], owner: &str, errors: &mut Vec<String>| -> Vec<Stack> {
            list.iter()
                .filter_map(|(id, n)| {
                    if *n == 0 {
                        errors.push(format!("{owner}: count of `{id}` is 0"));
                    }
                    item(fc, id, owner, errors).map(|item| Stack { item, count: *n })
                })
                .collect()
        };

        // Buildings.
        for (i, d) in buildings.into_iter().enumerate() {
            let owner = format!("building `{}`", d.id);
            if d.size.0 == 0 || d.size.1 == 0 {
                errors.push(format!("{owner}: size must be at least 1 x 1"));
            }
            let body = mats.find(&d.body).unwrap_or_else(|| {
                errors.push(format!("{owner}: body material `{}` does not exist", d.body));
                MaterialId::AIR
            });
            let ports = d
                .ports
                .iter()
                .map(|p| {
                    if p.tile.0 >= d.size.0 || p.tile.1 >= d.size.1 {
                        errors.push(format!("{owner}: port at tile {:?} is outside the building", p.tile));
                    }
                    Port {
                        kind: p.kind,
                        tile: p.tile,
                        side: p.side,
                        filter: p.filter.iter().filter_map(|f| item(&fc, f, &owner, errors)).collect(),
                        name: p.name.clone(),
                    }
                })
                .collect();
            let part = fc.part_ids[&d.id];
            if fc.building_ids.insert(d.id.clone(), BuildingKindId(i as u16)).is_some() {
                errors.push(format!("building id `{}` is used twice", d.id));
            }
            fc.buildings.push(Building {
                id: d.id,
                name: d.name,
                description: d.description,
                kind: d.kind,
                size: d.size,
                layer: d.layer,
                tier: d.tier,
                body,
                hit_points: d.hit_points,
                max_temp: d.max_temp,
                ports,
                power: d.power,
                crafts: d.crafts,
                speed: d.speed,
                params: d.params,
                part,
            });
        }

        // Recipes.
        let categories: std::collections::HashSet<&str> =
            fc.buildings.iter().flat_map(|b| b.crafts.iter().map(|s| s.as_str())).chain(["hand"]).collect();
        let mut recipes_out = vec![];
        for d in &recipes {
            let owner = format!("recipe `{}`", d.id);
            if !id_ok(&d.id) {
                errors.push(format!("{owner}: use only a-z, 0-9 and _ in the id"));
            }
            if !categories.contains(d.category.as_str()) {
                errors.push(format!("{owner}: no building crafts category `{}`", d.category));
            }
            if d.outputs.is_empty() {
                errors.push(format!("{owner}: needs at least one output"));
            }
            if d.time <= 0.0 {
                errors.push(format!("{owner}: time must be above 0"));
            }
            let inputs = stacks(&fc, &d.inputs, &owner, errors);
            let outputs = stacks(&fc, &d.outputs, &owner, errors);
            let byproducts = d
                .byproducts
                .iter()
                .filter_map(|b| {
                    if !(0.0..=1.0).contains(&b.chance) {
                        errors.push(format!("{owner}: byproduct chance must be in 0..=1"));
                    }
                    item(&fc, &b.item, &owner, errors).map(|i| (Stack { item: i, count: b.count }, b.chance))
                })
                .collect();
            let first = outputs.first().map(|s| s.item);
            let name = d.name.clone().unwrap_or_else(|| match first {
                Some(ItemRef::Part(p)) => fc.parts[p.0 as usize].name.clone(),
                Some(ItemRef::Material(m)) => mats.names[m.index()].clone(),
                None => d.id.clone(),
            });
            let group = d.group.clone().unwrap_or_else(|| match first {
                Some(ItemRef::Part(p)) => fc.parts[p.0 as usize].category.clone(),
                _ => "intermediate".into(),
            });
            if fc.recipe_ids.insert(d.id.clone(), RecipeId(recipes_out.len() as u16)).is_some() {
                errors.push(format!("recipe id `{}` is used twice", d.id));
            }
            recipes_out.push(Recipe {
                id: d.id.clone(),
                name,
                category: d.category.clone(),
                hand: d.hand || d.category == "hand",
                inputs,
                outputs,
                byproducts,
                time: d.time,
                tier: d.tier,
                min_temp: d.min_temp,
                group,
                unlocked_by: None,
            });
        }
        fc.recipes = recipes_out;

        // Technologies.
        for (i, d) in techs.iter().enumerate() {
            if fc.tech_ids.insert(d.id.clone(), TechId(i as u16)).is_some() {
                errors.push(format!("tech id `{}` is used twice", d.id));
            }
        }
        for d in techs {
            let owner = format!("tech `{}`", d.id);
            let requires = d
                .requires
                .iter()
                .filter_map(|r| {
                    let t = fc.tech_ids.get(r).copied();
                    if t.is_none() {
                        errors.push(format!("{owner}: requires `{r}`, which does not exist"));
                    }
                    t
                })
                .collect();
            let mut unlocks = vec![];
            for r in &d.unlocks {
                match fc.recipe_ids.get(r).copied() {
                    Some(rid) => {
                        let this = fc.tech_ids[&d.id];
                        let slot = &mut fc.recipes[rid.0 as usize].unlocked_by;
                        if slot.is_some() {
                            errors.push(format!("{owner}: recipe `{r}` is already unlocked by another technology"));
                        }
                        *slot = Some(this);
                        unlocks.push(rid);
                    }
                    None => errors.push(format!("{owner}: unlocks `{r}`, which is not a recipe")),
                }
            }
            let kits = stacks(&fc, &d.kits, &owner, errors);
            fc.techs.push(Tech {
                id: d.id,
                name: d.name,
                description: d.description,
                tier: d.tier,
                requires,
                kits,
                units: d.units,
                unit_time: d.unit_time,
                discoveries: d.discoveries,
                discovery_points: d.discovery_points,
                unlocks,
                effects: d.effects,
            });
        }
        // No cycles in `requires`.
        let n = fc.techs.len();
        let mut state = vec![0u8; n]; // 0 new, 1 visiting, 2 done
        fn visit(i: usize, techs: &[Tech], state: &mut [u8]) -> bool {
            if state[i] == 1 {
                return false;
            }
            if state[i] == 2 {
                return true;
            }
            state[i] = 1;
            for r in &techs[i].requires {
                if !visit(r.0 as usize, techs, state) {
                    return false;
                }
            }
            state[i] = 2;
            true
        }
        for i in 0..n {
            if !visit(i, &fc.techs, &mut state) {
                errors.push(format!("tech `{}` is part of a loop in `requires`", fc.techs[i].id));
                break;
            }
        }

        // Milestones.
        for d in milestones {
            let owner = format!("milestone {}", d.stage);
            let deliver = stacks(&fc, &d.deliver, &owner, errors);
            fc.milestones.push(Milestone {
                stage: d.stage,
                name: d.name,
                description: d.description,
                deliver,
                unlocks_tier: d.unlocks_tier,
            });
        }
        fc.milestones.sort_by_key(|m| m.stage);
        fc
    }
}
