extern crate tcod;
extern crate rand;

use std::cmp;
use tcod::console::*;
use tcod::colors::{self, Color};
use tcod::map::{Map as FovMap, FovAlgorithm};
use tcod::input::{self, Event, Key, Mouse};
use rand::Rng;

const SCREEN_WIDTH: i32 = 80;
const SCREEN_HEIGHT: i32 = 50;
const LIMIT_FPS: i32 = 20;
const MAP_WIDTH: i32 = 80;
const MAP_HEIGHT: i32 = 43;
const ROOM_MAX_SIZE: i32 = 10;
const ROOM_MIN_SIZE: i32 = 6;
const MAX_ROOMS: i32 = 30;
const FOV_ALGO: FovAlgorithm = FovAlgorithm::Basic;
const FOV_LIGHT_WALLS: bool = true;
const TORCH_RADIUS: i32 = 10;
const MAX_ROOM_MONSTERS: i32 = 3;
//sizes and coordinates relevant for the GUI
const BAR_WIDTH: i32 = 20;
const PANEL_HEIGHT: i32 = 7;
const PANEL_Y: i32 = SCREEN_HEIGHT - PANEL_HEIGHT;
// player will always be the first object
const PLAYER: usize = 0;
const MSG_X: i32 = BAR_WIDTH + 2;
const MSG_WIDTH: i32 = SCREEN_WIDTH - BAR_WIDTH - 2;
const MSG_HEIGHT: usize = PANEL_HEIGHT as usize - 1;
const MAX_ROOM_ITEMS: i32 = 2;
const INVENTORY_WIDTH: i32 = 50;
const HEAL_AMOUNT: i32 = 4;
const LIGHTNING_DAMAGE: i32 = 5;
const LIGHTNING_RANGE: i32 = 5;
const CONFUSE_RANGE: i32 = 7;
const CONFUSE_NUM_TURNS: i32 = 4;
const FIREBALL_RADIUS: i32 = 3;
const FIREBALL_DAMAGE: i32 = 5;

const COLOR_DARK_WALL: Color = Color { r: 0, g: 0, b: 100 };
const COLOR_LIGHT_WALL: Color = Color { r: 130, g: 110, b: 50 };
const COLOR_DARK_GROUND: Color = Color { r: 50, g: 50, b: 150 };
const COLOR_LIGHT_GROUND: Color = Color { r: 200, g: 180, b: 50 };

struct Tcod {
    root: Root,
    con: Offscreen,
    panel: Offscreen,
    fov: FovMap,
    mouse: Mouse,
}

struct Game {
    map: Map,
    log: Messages,
    inventory: Vec<Object>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Item {
    Heal,
    Lightning,
    Fireball,
    Confusion,
}

enum UseResult {
    UsedUp,
    Cancelled,
}

/// add to player's inventory and remove from the map
fn pick_item_up(object_id: usize, objects: &mut Vec<Object>, game: &mut Game) {
    if game.inventory.len() >= 26 {
        message(&mut game.log,
                format!("Your inventory is full, cannot pick up {}", objects[object_id].name),
                colors::RED);
    } else {
        let item = objects.swap_remove(object_id);
        message(&mut game.log,
                format!("You picked up {}!", item.name),
                colors::GREEN);
        game.inventory.push(item);
    }
}

fn drop_item(inventory_id: usize,
             game: &mut Game,
             objects: &mut Vec<Object>) {
    let mut item = game.inventory.remove(inventory_id);
    item.set_pos(objects[PLAYER].x, objects[PLAYER].y);
    message(&mut game.log,
            format!("You dropped a {}", item.name),
            colors::YELLOW);
    objects.push(item);
}

fn use_item(tcod: &mut Tcod, inventory_id: usize, game: &mut Game, objects:
            &mut [Object]) {
    use Item::*;
    // just call the "use function" if it is defined
    if let Some(item) = game.inventory[inventory_id].item {
        let on_use: fn(&mut Tcod, usize, &mut [Object], &mut Game) -> UseResult = match item {
            Heal => cast_heal,
            Lightning => cast_lightning,
            Fireball => cast_fireball,
            Confusion => cast_confuse,
        };
        match on_use(tcod, inventory_id, objects, game) {
            UseResult::UsedUp => {
                // destroy after use, unless it was cancelled for some reason
                game.inventory.remove(inventory_id);
            }
            UseResult::Cancelled => {
                message(&mut game.log, "Cancelled", colors::WHITE);
            }
        }
    } else {
        message(&mut game.log,
                format!("The {} cannot be used.", game.inventory[inventory_id].name),
                colors::WHITE);
    }
}

/// Heals the player
fn cast_heal(_tcod: &mut Tcod, _inventory_id: usize, objects: &mut [Object],
             game: &mut Game) -> UseResult
{
    if let Some(fighter) = objects[PLAYER].fighter {
        if fighter.hp == fighter.max_hp {
            message(&mut game.log, "You are already at full health", colors::RED);
            return UseResult::Cancelled;
        }
        message(&mut game.log, "Your wounds start to feel better!", colors::LIGHT_VIOLET);
        objects[PLAYER].heal(HEAL_AMOUNT);
        return UseResult::UsedUp;
    }
    UseResult::Cancelled
}

/// Damages nearest enemy
fn cast_lightning(tcod: &mut Tcod, _inventory_id: usize, objects: &mut [Object],
                  game: &mut Game) -> UseResult {
    // find the closest enemy (inside a maximum range) and damage it
    let monster_id = closest_monster(tcod, LIGHTNING_RANGE, objects);
    if let Some(monster_id) = monster_id {
        // zap it!
        message(&mut game.log,
                format!("A lightning bolt strikes {} with a loud thunder! \
                         The damage is {} hit points.",
                         objects[monster_id].name, LIGHTNING_DAMAGE),
                colors::LIGHT_BLUE);
                objects[monster_id].take_damage(LIGHTNING_DAMAGE, game);
                UseResult::UsedUp
    } else {
        // no enemy found within maximum range
        message(&mut game.log, "No enemy is close enough to strike.", colors::RED);
        UseResult::Cancelled
    }
}

fn cast_fireball(tcod: &mut Tcod, _inventory_id: usize, objects: &mut [Object],
                 game: &mut Game) -> UseResult {
    // ask the player for a target tile to throw a fireball at
    message(&mut game.log,
            "Left-click a target tile for the fireball, or right-click to cancel.",
            colors::LIGHT_CYAN);
    let (x, y) = match target_tile(tcod, objects, game, None) {
        Some(tile_pos) => tile_pos,
        None => return UseResult::Cancelled,
    };
    message(&mut game.log,
            format!("The fireball explodes burning everyting within {} tiles!",
                    FIREBALL_RADIUS),
            colors::ORANGE);

    for obj in objects {
        if obj.distance(x, y) <= FIREBALL_RADIUS as f32 && obj.fighter.is_some() {
            message(&mut game.log,
                    format!("The {} gets burned for {} hit points.", obj.name, FIREBALL_DAMAGE),
                    colors::ORANGE);
            obj.take_damage(FIREBALL_DAMAGE, game);
        }
    }

    UseResult::UsedUp
}

/// Confuses nearest enemy
fn cast_confuse(tcod: &mut Tcod, _inventory_id: usize, objects: &mut [Object],
                game: &mut Game) -> UseResult {
    // ask the player for a target to confuse
    message(&mut game.log,
            "Left-click an enemy to confuse it, or right-click to cancel.",
            colors::LIGHT_CYAN);
    let monster_id = target_monster(tcod, objects, game, Some(CONFUSE_RANGE as f32));
    if let Some(monster_id) = monster_id {
        let old_ai = objects[monster_id].ai.take().unwrap_or(Ai::Basic);
        // replace the monster's AI with a confused one
        // after some turn, the old AI is restored
        objects[monster_id].ai = Some(Ai::Confused {
            previous_ai: Box::new(old_ai),
            num_turns: CONFUSE_NUM_TURNS,
        });
        message(&mut game.log,
                format!("The {} is confused, he wanders around!",
                        objects[monster_id].name),
                colors::LIGHT_GREEN);
        UseResult::UsedUp
    } else {    // no enemy found within maximum range
        message(&mut game.log, "No enemy is close enough", colors::RED);
        UseResult::Cancelled
    }
}

type Messages = Vec<(String, Color)>;

fn message<T: Into<String>>(messages: &mut Messages, message: T, color: Color) {
    // If the buffer is full, remove the first message to make room for the new one
    if messages.len() == MSG_HEIGHT {
        messages.remove(0);
    }
    // add new line as tuple, with text and color
    messages.push((message.into(), color));
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PlayerAction {
    TookTurn,
    DidntTakeTurn,
    Exit,
}

/// Mutably borrow two *separate* elements from the given slice.
/// Panics when the indexes are equal or out of bounds.
fn mut_two<T>(first_index: usize, second_index: usize, items: &mut [T]) -> (&mut T, &mut T) {
    assert!(first_index != second_index);
    let split_at_index = cmp::max(first_index, second_index);
    let (first_slice, second_slice) = items.split_at_mut(split_at_index);
    if first_index < second_index {
        (&mut first_slice[first_index], &mut second_slice[0])
    } else {
        (&mut second_slice[0], &mut first_slice[second_index])
    }
}

#[derive(Clone, Copy, Debug)]
struct Tile {
    blocked: bool,
    explored: bool,
    block_sight: bool,
}

impl Tile {
    pub fn empty() -> Self {
        Tile{blocked: false, explored: false, block_sight: false}
    }

    pub fn wall() -> Self {
        Tile{blocked: true, explored: false,  block_sight: true}
    }
}

type Map = Vec<Vec<Tile>>;

/// fill map with "blocked" tiles
fn make_map(objects: &mut Vec<Object>) -> Map {
    let mut map = vec![vec![Tile::wall(); MAP_HEIGHT as usize]; MAP_WIDTH as usize];
    let mut rooms = vec![];

    for _ in 0..MAX_ROOMS {
        // random width and height
        let w = rand::thread_rng().gen_range(ROOM_MIN_SIZE, ROOM_MAX_SIZE + 1);
        let h = rand::thread_rng().gen_range(ROOM_MIN_SIZE, ROOM_MAX_SIZE + 1);
        // random position without going out of the boundary of the map
        let x = rand::thread_rng().gen_range(0, MAP_WIDTH - w);
        let y = rand::thread_rng().gen_range(0, MAP_HEIGHT - h);

        let new_room = Rect::new(x, y, w, h);

        // run through the other rooms and see if they intersect with this one
        let failed = rooms.iter().any(|other_room| new_room.intersects_with(other_room));
        if !failed {
            // this means there are no intersections, so this room is valid

            // "paint" it to the map's tiles
            create_room(new_room, &mut map);

            // add some content to this room, such as monsters
            place_objects(new_room, &map, objects);

            // center coordinates of the new room, will be useful later
            let (new_x, new_y) = new_room.center();

            if rooms.is_empty() {
                // this is the first room, where the player starts at
                objects[PLAYER].set_pos(new_x, new_y);
            } else {
                // all rooms after the first
                // connect it to the previous room with a tunnel

                // center coordinates of the previous room
                let (prev_x, prev_y) = rooms[rooms.len() - 1].center();

                // draw a coin (random bool value -- either true or false)
                if rand::random() {
                    // first move horizontally, then vertically
                    create_h_tunnel(prev_x, new_x, prev_y, &mut map);
                    create_v_tunnel(prev_y, new_y, new_x, &mut map);
                } else {
                    // first move bertically, then horizontally
                    create_v_tunnel(prev_y, new_y, prev_x, &mut map);
                    create_h_tunnel(prev_x, new_x, new_y, &mut map);
                }
            }

            // finally, append the new room to the list`
            rooms.push(new_room);
        }
    }

    map
}

/// combat-related properties and methods for monster, player, NPC).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fighter {
    max_hp: i32,
    hp: i32,
    defense: i32,
    power: i32,
    on_death: DeathCallback,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DeathCallback {
    Player,
    Monster,
}

impl DeathCallback {
    fn callback(self, object: &mut Object, game: &mut Game) {
        use DeathCallback::*;
        let callback: fn(&mut Object, &mut Game) = match self {
            Player => player_death,
            Monster => monster_death,
        };
        callback(object, game);
    }
}

fn player_death(player: &mut Object, game: &mut Game) {
    // the game has ended!
    message(&mut game.log, "You died!", colors::WHITE);

    // transform player into a corpse!
    player.char = '%';
    player.color = colors::DARK_RED;
}

fn monster_death(monster: &mut Object, game: &mut Game) {
    // transform into a corpse. It doesn't block, can be attacked and doesn't
    // move
    message(&mut game.log, format!("{} is dead!", monster.name), colors::GREEN);
    monster.char = '%';
    monster.color = colors::DARK_RED;
    monster.blocks = false;
    monster.fighter = None;
    monster.ai = None;
    monster.name = format!("remains of {}", monster.name);
}

#[derive(Clone, Debug, PartialEq)]
enum Ai{
    Basic,
    Confused{previous_ai: Box<Ai>, num_turns: i32},
}

// Generic object: Player, Monster, Item, Stairs
struct Object {
    name: String,
    blocks: bool,
    alive: bool,
    x: i32,
    y: i32,
    char: char,
    color: Color,
    fighter: Option<Fighter>,
    ai: Option<Ai>,
    item: Option<Item>,
}

impl Object {
    pub fn pos(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    pub fn set_pos(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
    }

    pub fn new(x: i32, y: i32, char: char, name: &str, color: Color, blocks: bool) -> Self {
        Object {
            x: x,
            y: y,
            char: char,
            name: name.into(),
            color: color,
            blocks: blocks,
            alive: false,
            fighter: None,
            ai: None,
            item: None,
        }
    }

    /// set the color and then draw the character that represents this object at its position
    pub fn draw(&self, con: &mut Console) {
        con.set_default_background(self.color);
        con.put_char(self.x, self.y, self.char, BackgroundFlag::None);
    }

    /// Erase the character that represents this object
    pub fn clear(&self, con: &mut Console) {
        con.put_char(self.x, self.y, ' ', BackgroundFlag::None);
    }

    /// return the distance to another object
    pub fn distance_to(&self, other: &Object) -> f32 {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        ((dx.pow(2) + dy.pow(2)) as f32).sqrt()
    }

    pub fn take_damage(&mut self, damage: i32, game: &mut Game) {
        // apply damage if possible
        if let Some(fighter) = self.fighter.as_mut() {
            if damage > 0 {
                fighter.hp -= damage;
            }
        }
        // check for death
        if let Some(fighter) = self.fighter {
            if fighter.hp <= 0 {
                self.alive = false;
                fighter.on_death.callback(self, game);
            }
        }
    }

    pub fn attack(&mut self, target: &mut Object, game: &mut Game) {
        // simple attack formula
        let damage = self.fighter.map_or(0, |f| f.power) - target.fighter.map_or(0, |f| f.defense);
        if damage > 0 {
            // make the target take some damage
            message(&mut game.log, format!("{} attacks {} for {} hit points.",
                    self.name, target.name, damage), colors::WHITE);
            target.take_damage(damage, game);
        } else {
            message(&mut game.log, format!("{} attacks {} but it has no effect!",
                    self.name, target.name), colors::WHITE);
        }
    }

    /// heal by the given amount, without going over the maximum
    pub fn heal(&mut self, amount: i32) {
        if let Some(ref mut fighter) = self.fighter {
            fighter.hp += amount;
            if fighter.hp > fighter.max_hp {
                fighter.hp = fighter.max_hp;
            }
        }
    }

    /// returns the distance fromm some coordinates
    pub fn distance(&self, x: i32, y: i32) -> f32  {
        (((x - self.x).pow(2) + (y - self.x).pow(2)) as f32).sqrt()
    }
}

fn move_by(id: usize, dx: i32, dy: i32, game: &Game, objects: &mut [Object]) {
    let (x, y) = objects[id].pos();
    if !is_blocked(x + dx, y + dy, &game.map, objects) {
        objects[id].set_pos(x + dx, y + dy);
    }
}

fn move_towards(id: usize, target_x: i32, target_y: i32, game: &Game,
                objects: &mut [Object]) {
    // vector from this object to the target, and distance
    let dx = target_x - objects[id].x;
    let dy = target_y - objects[id].y;
    let distance = ((dx.pow(2) + dy.pow(2)) as f32).sqrt();

    // normalize it to length 1 (preserving the direction), then round it and
    // convert to integer so the movement is restricted to the map grid
    let dx = (dx as f32 / distance).round() as i32;
    let dy = (dy as f32 / distance).round() as i32;
    move_by(id, dx, dy, game, objects);
}

/// handle player movements and attacks
fn player_move_or_attack(dx: i32, dy: i32, game: &mut Game,
                         objects: &mut [Object]) {
    // the player coordinates moving to/attacking
    let x = objects[PLAYER].x + dx;
    let y = objects[PLAYER].y + dy;

    // try to find an attackable object there
    let target_id = objects.iter().position(|object| {
        object.fighter.is_some() && object.pos() == (x, y)
    });

    // attack if there is a target. Otherwise, move
    match target_id {
        Some(target_id) => {
            let (player, target) = mut_two(PLAYER, target_id, objects);
            player.attack(target, game);
        }
        None => {
            move_by(PLAYER, dx, dy, game, objects);
        }
    }
}

fn ai_take_turn(monster_id: usize, game: &mut Game, objects: &mut [Object],
                fov_map: &FovMap) {
    use Ai::*;
    if let Some(ai) = objects[monster_id].ai.take() {
        let new_ai = match ai {
            Basic => ai_basic(monster_id, game, objects, fov_map),
            Confused{previous_ai, num_turns} => ai_confused(monster_id, game,
                                    objects, previous_ai, num_turns)
        };
        objects[monster_id].ai = Some(new_ai);
    }
}

fn ai_basic(monster_id: usize, game: &mut Game, objects: &mut [Object],
                fov_map: &FovMap) -> Ai {
    // a basic monster takes its turn. If you can see it, it can see you
    let (monster_x, monster_y) = objects[monster_id].pos();
    if fov_map.is_in_fov(monster_x, monster_y) {
        if objects[monster_id].distance_to(&objects[PLAYER]) >= 2.0 {
            // move towards player if far away
            let (player_x, player_y) = objects[PLAYER].pos();
            move_towards(monster_id, player_x, player_y, game, objects);
        } else if objects[PLAYER].fighter.map_or(false, |f| f.hp > 0) {
            // close enough to attack! (if the player is still alive.)
            let (monster, player) = mut_two(monster_id, PLAYER, objects);
            monster.attack(player, game);
        }
    }
    Ai::Basic
}

fn ai_confused(monster_id: usize, game: &mut Game, objects: &mut [Object],
               previous_ai: Box<Ai>, num_turns: i32)
                    -> Ai {
    // still confused
    if num_turns >= 0 {
        // move in a random direction, decrease the number of turns confused
        move_by(monster_id,
                rand::thread_rng().gen_range(-1, 2),
                rand::thread_rng().gen_range(-1, 2),
                game,
                objects);
        Ai::Confused{previous_ai: previous_ai, num_turns: num_turns - 1}
    } else {    // restore the previous AI
        message(&mut game.log, format!("The {} is no longer confused!",
                                       objects[monster_id].name),
                colors::RED);
        *previous_ai
    }
}

/// find the closest enemy, up to a maximum range within player's FOV
fn closest_monster(tcod: &Tcod, max_range: i32, objects: &mut [Object])
        -> Option<usize> {
    let mut closest_enemy = None;
    // start with slightly more than maximum range
    let mut closest_dist = (max_range + 1) as f32;

    for (id, object) in objects.iter().enumerate() {
        if (id != PLAYER) && object.fighter.is_some() && object.ai.is_some() &&
            tcod.fov.is_in_fov(object.x, object.y)
        {
            // calculate the distance between the player and this object
            let dist = objects[PLAYER].distance_to(object);
            // it's closer. Save the enemy
            if dist < closest_dist {
                closest_enemy = Some(id);
                closest_dist = dist;
            }
        }
    }
    closest_enemy
}

fn place_objects(room: Rect, map: &Map, objects: &mut Vec<Object>) {
    // choose random number of monster
    let num_monsters = rand::thread_rng().gen_range(0, MAX_ROOM_MONSTERS + 1);

    for _ in 0..num_monsters {
        // chose random spot for this monster
        let x = rand::thread_rng().gen_range(room.x1 + 1, room.x2);
        let y = rand::thread_rng().gen_range(room.y1 + 1, room.y2);

        // only place it if the tile is not blocked
        if !is_blocked(x, y, map, objects) {
            // generate the monsters
            let mut monster = if rand::random::<f32>() < 0.8 {  // 80% chance to create an orc
                // create orc
                let mut orc = Object::new(x, y, 'o', "orc", colors::DESATURATED_GREEN, true);
                orc.fighter = Some(Fighter{max_hp: 4, hp: 4, defense: 0, power: 2,
                                           on_death: DeathCallback::Monster});
                orc.ai = Some(Ai::Basic);
                orc
            } else {
                // create troll
                let mut troll = Object::new(x, y, 'T', "troll", colors::DARKER_GREEN, true);
                troll.fighter = Some(Fighter{max_hp: 5, hp: 5, defense: 0, power: 2,
                                             on_death: DeathCallback::Monster});
                troll.ai = Some(Ai::Basic);
                troll
            };
            monster.alive = true;
            objects.push(monster);
        }
    }

    // choose random number of items
    let num_items = rand::thread_rng().gen_range(0, MAX_ROOM_ITEMS + 1);

    for _ in 0..num_items {
        // choose random spot for this item
        let x = rand::thread_rng().gen_range(room.x1 + 1, room.x2);
        let y = rand::thread_rng().gen_range(room.y1 + 1, room.y2);

        // only place the item if the tile is not blocked
        if !is_blocked(x, y, map, objects) {
            let dice = rand::random::<f32>();
            let item = if dice < 0.4 {
                // create healing potion (40% chance)
                let mut object = Object::new(x, y, '!', "healing potion",
                                            colors::VIOLET, false);
                object.item = Some(Item::Heal);
                object
            } else if dice < 0.4 + 0.2 {
                // create a lightning bolt scroll (20% chance)
                let mut object = Object::new(x, y, '#', "scroll of lightning",
                                             colors::LIGHT_YELLOW, false);
                object.item = Some(Item::Lightning);
                object
            } else if dice < 0.4 + 0.2 + 0.2 {
                // create a fireball scroll (20% chance)
                let mut object = Object::new(x, y, 'F', "scroll of fireball",
                                             colors::LIGHT_YELLOW, false);
                object.item = Some(Item::Fireball);
                object
            } else {
                // create a confusion scroll (20% chance)
                let mut object = Object::new(x, y, 'C', "scroll of confusion",
                                             colors::LIGHT_ORANGE, false);
                object.item = Some(Item::Confusion);
                object
            };
            objects.push(item);
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x1: x, y1: y, x2: x + w, y2: y + h }
    }

    pub fn center(&self) -> (i32, i32) {
        let center_x = (self.x1 + self.x2) / 2;
        let center_y = (self.y1 + self.y2) / 2;
        (center_x, center_y)
    }

    pub fn intersects_with(&self, other: &Rect) -> bool {
        (self.x1 <= other.x2) && (self.x2 >= other.x1) &&
            (self.y1 <= other.y2) && (self.y2 >= other.y1)
    }
}

fn create_room(room: Rect, map: &mut Map) {
    for x in (room.x1 + 1)..room.x2 {
        for y in (room.y1 + 1)..room.y2 {
            map[x as usize][y as usize] = Tile::empty();
        }
    }
}

fn create_h_tunnel(x1: i32, x2: i32, y: i32, map: &mut Map) {
    for x in cmp::min(x1, x2)..(cmp::max(x1,x2) + 1) {
        map[x as usize][y as usize] = Tile::empty();
    }
}

fn create_v_tunnel(y1: i32, y2: i32, x: i32, map: &mut Map) {
    for y in cmp::min(y1, y2)..(cmp::max(y1, y2) + 1) {
        map[x as usize][y as usize] = Tile::empty();
    }
}

fn is_blocked(x: i32, y: i32, map: &Map, objects: &[Object]) -> bool {
    // first test the map tile
    if map[x as usize][y as usize].blocked {
        return true;
    }
    // now check for any blocking objects
    objects.iter().any(|object| {
        object.blocks && object.pos() == (x, y)
    })
}

fn render_bar(panel: &mut Offscreen,
              x: i32,
              y: i32,
              total_width: i32,
              name: &str,
              value: i32,
              maximum: i32,
              bar_color: Color,
              back_color: Color)
{
    // render a bar for HP, experience etc. Calculate width of the bar
    let bar_width = (value as f32 / maximum as f32 * total_width as f32) as i32;

    // reneder the background first
    panel.set_default_background(back_color);
    panel.rect(x, y, total_width, 1, false, BackgroundFlag::Screen);

    // now render the bar on top
    panel.set_default_background(bar_color);
    if bar_width > 0 {
        panel.rect(x, y, bar_width, 1, false, BackgroundFlag::Screen);
    }

    // add centered text with values
    panel.set_default_background(colors::WHITE);
    panel.print_ex(x + total_width / 2, y, BackgroundFlag::None, TextAlignment::Center,
                   &format!("{}: {}/{}", name, value, maximum));
}

fn menu<T: AsRef<str>>(header: &str, options: &[T], width: i32,
                       root: &mut Root) -> Option<usize> {
    assert!(options.len() <= 26, "Cannot have a menu more than the limit");

    // calculate the total height for the header (after auto-wrap) and one line
    // per option
    let header_height = root.get_height_rect(0, 0, width, SCREEN_HEIGHT, header);
    let height = options.len() as i32 + header_height;

    // create an off-screen console that represent's the menu;s window
    let mut window = Offscreen::new(width, height);

    // print the header, with auto-wrap
    window.set_default_foreground(colors::WHITE);
    window.print_rect_ex(0, 0, width, height, BackgroundFlag::None,
                         TextAlignment::Left, header);

    // printout the options
    for (index, option_text) in options.iter().enumerate() {
        let menu_letter = (b'a' + index as u8) as char;
        let text = format!("({}) {}", menu_letter, option_text.as_ref());
        window.print_ex(0, header_height + index as i32,
                        BackgroundFlag::None, TextAlignment::Left, text);
    }

    // blit the contents of "window" to the root console
    let x = SCREEN_WIDTH / 2 - width / 2;
    let y = SCREEN_HEIGHT / 2 - height / 2;
    tcod::console::blit(&mut window, (0, 0), (width, height), root, (x, y),
                        1.0, 0.7);
    
    // present the root console to the player and wait for key press
    root.flush();
    let key = root.wait_for_keypress(true);

    // convert ASCII code to an index. If it corresponds to an action, return it
    if key.printable.is_alphabetic() {
        let index = key.printable.to_ascii_lowercase() as usize - 'a' as usize;
        if index < options.len() {
            Some(index)
        } else {
            None
        }
    } else {
        None
    }
}

/// have a menu with each item of the inventory as an option
fn inventory_menu(inventory: &[Object], header: &str, root: &mut Root) -> Option<usize> {
    let options = if inventory.len() == 0 {
        vec!["Intenvtory is empty.".into()]
    } else {
        inventory.iter().map(|item| { item.name.clone() }).collect()
    };

    let inventory_index = menu(header, &options, INVENTORY_WIDTH, root);

    // if an item was chosen, return it
    if inventory.len() > 0 {
        inventory_index
    } else {
        None
    }
}

/// Draw all of the objects in the list
fn render_all(tcod: &mut Tcod, objects: &[Object], game: &mut Game, 
              fov_recompute: bool) {
    if fov_recompute {
        // recompute FOV if needed (the player moved or something)
        let player = &objects[PLAYER];
        tcod.fov.compute_fov(player.x, player.y, TORCH_RADIUS, FOV_LIGHT_WALLS, FOV_ALGO);
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                let visible = tcod.fov.is_in_fov(x, y);
                let wall = game.map[x as usize][y as usize].block_sight;
                let color = match (visible, wall) {
                    // outside of field of view:
                    (false, true) => COLOR_DARK_WALL,
                    (false, false) => COLOR_DARK_GROUND,
                    // inside fov:
                    (true, true) => COLOR_LIGHT_WALL,
                    (true, false) => COLOR_LIGHT_GROUND,
                };
                let explored = &mut game.map[x as usize][y as usize].explored;
                if visible {
                    // since it's visible, explore it
                    *explored = true;
                }
                if *explored {
                    // show explored tile only (any visible tile is eplored already)
                    tcod.con.set_char_background(x, y, color, BackgroundFlag::Set);
                }
            }
        }
    }

    let mut to_draw: Vec<_> = objects.iter().filter(|o| tcod.fov.is_in_fov(o.x, o.y)).collect();
    // sort so that non-blocking objects come first
    to_draw.sort_by(|o1, o2| { o1.blocks.cmp(&o2.blocks) });
    // draw the objects in the list
    for object in to_draw {
        object.draw(&mut tcod.con);
    }
    // blit contents of "con" to root console and present it
    blit(&mut tcod.con, (0, 0), (MAP_WIDTH, MAP_HEIGHT), &mut tcod.root, (0, 0), 1.0, 1.0);

    // prepare to render the GUI panel
    tcod.panel.set_default_background(colors::BLACK);
    tcod.panel.clear();

    // show the player's stats
    let hp = objects[PLAYER].fighter.map_or(0, |f| f.hp);
    let max_hp = objects[PLAYER].fighter.map_or(0, |f| f.max_hp);
    render_bar(&mut tcod.panel, 1, 1, BAR_WIDTH, "HP", hp, max_hp,
               colors::LIGHT_RED, colors::DARKER_RED);

    // display the object names under the mouse
    tcod.panel.set_default_foreground(colors::LIGHT_GREY);
    tcod.panel.print_ex(1, 0, BackgroundFlag::None, TextAlignment::Left,
                   get_names_under_mouse(tcod.mouse, objects, &tcod.fov));

    // print game messages
    let mut y = MSG_HEIGHT as i32;
    for &(ref msg, color) in game.log.iter().rev() {
        let msg_height = tcod.panel.get_height_rect(MSG_X, y, MSG_WIDTH, 0, msg);
        y -= msg_height;
        if y < 0 {
            break;
        }
        tcod.panel.set_default_foreground(color);
        tcod.panel.print_rect(MSG_X, y, MSG_WIDTH, 0, msg);
    }

    // blit contents of "con" to root console and present it
    blit(&mut tcod.panel, (0, 0), (SCREEN_WIDTH, PANEL_HEIGHT), &mut tcod.root,
         (0, PANEL_Y), 1.0, 1.0);
}

/// return a string with names of all objects under the mouse
fn get_names_under_mouse(mouse: Mouse, objects: &[Object], fov_map: &FovMap) -> String {
    let (x, y) = (mouse.cx as i32, mouse.cy as i32);

    // create a list with the names of all the objects at the mouse's
    // coordinates and in FOV
    let names = objects
         .iter()
         .filter(|obj| {obj.pos() == (x, y) && fov_map.is_in_fov(obj.x, obj.y)})
         .map(|obj| obj.name.clone())
         .collect::<Vec<_>>();

    // separate by commas
    names.join(", ")
}

/// return the position of a tile left-clicked in player's POV (optionally in =
/// a range) or (None, None) if right-clicked.
fn target_tile(tcod: &mut Tcod,
               objects: &[Object],
               game: &mut Game,
               max_range: Option<f32>)
               -> Option<(i32, i32)> {
    use tcod::input::KeyCode::Escape;
    loop {
        // render the screen. this erase the inventory and shows the names of
        // the objects under the mouse.
        tcod.root.flush();
        let event = input::check_for_event(input::KEY_PRESS | input::MOUSE).map(|e| e.1);
        let mut key = None;
        match event {
            Some(Event::Mouse(m)) => tcod.mouse = m,
            Some(Event::Key(k)) => key = Some(k),
            None => {}
        }
        render_all(tcod, objects, game, false);

        let (x, y) = (tcod.mouse.cx as i32, tcod.mouse.cy as i32);

        // accept the target if the player clicked in FOV, and in case of range is
        // is specified, if it's in that range
        let in_fov = (x < MAP_WIDTH) && (y < MAP_HEIGHT) && tcod.fov.is_in_fov(x, y);
        let in_range = max_range.map_or(
            true, |range| objects[PLAYER].distance(x, y) <= range);
        if tcod.mouse.lbutton_pressed && in_fov && in_range {
            return Some((x, y))
        }

        let escape = key.map_or(false, |k| k.code == Escape);
        if tcod.mouse.rbutton_pressed || escape {
            return None // cancel if the player right-clicked or pressed Esc
        }
    }
}

/// returns a clicked monster inside FOV up to a range, or None if right-clicked
fn target_monster(tcod: &mut Tcod,
                  objects: &[Object],
                  game: &mut Game,
                  max_range: Option<f32>)
                  -> Option<usize> {
    loop {
        match target_tile(tcod, objects, game, max_range) {
            Some((x, y)) => {
                // return the first clicked monster, otherwise continue looping
                for (id, obj) in objects.iter().enumerate() {
                    if obj.pos() == (x, y) && obj.fighter.is_some() && id != PLAYER {
                        return Some(id)
                    }
                }
            }
            None => return None,
        }
    }
}

fn handle_keys(key: Key, tcod: &mut Tcod, objects: &mut Vec<Object>,
               game: &mut Game) -> PlayerAction {
    use PlayerAction::*;
    use tcod::input::Key;
    use tcod::input::KeyCode::*;

    let player_alive = objects[PLAYER].alive;
    match (key, player_alive) {
        (Key { code: Escape, .. }, _) => Exit, // exit game
        (Key { code: Enter, alt: true, .. }, _) => {
            // Alt+Enter: toggle fullscreen
            let fullscreen = tcod.root.is_fullscreen();
            tcod.root.set_fullscreen(!fullscreen);
            DidntTakeTurn
        }
        // movement keys
        (Key { code: Up, .. }, true) => {
            player_move_or_attack(0, -1, game, objects);
            TookTurn
        }
        (Key { code: Down, .. }, true) => {
            player_move_or_attack(0, 1, game, objects);
            TookTurn
        }
        (Key { code: Left, .. }, true) => {
            player_move_or_attack(-1, 0, game, objects);
            TookTurn
        }
        (Key { code: Right, .. }, true) => {
            player_move_or_attack(1, 0, game, objects);
            TookTurn
        }
        (Key { printable: 'g', .. }, true) => {
            // pick up an item
            let item_id = objects.iter().position(|object| {
                object.pos() == objects[PLAYER].pos() && object.item.is_some()
            });
            if let Some(item_id) = item_id {
                pick_item_up(item_id, objects, game);
            }
            DidntTakeTurn
        }
        (Key { printable: 'i', .. }, true) => {
            // show inventory. If an item is selected, use it
            let inventory_index = inventory_menu(
                &mut game.inventory,
                "Press the key next to an item to use it, or any other to cancel.\n", 
                &mut tcod.root);
            if let Some(inventory_index) = inventory_index {
                use_item(tcod, inventory_index, game, objects);
            }
            DidntTakeTurn
        }
        (Key {printable: 'd', .. }, true) => {
            // show the inventory; if an item is selected, drop it
            let inventory_index = inventory_menu(
                &mut game.inventory,
                "Press the key next to an item to drop it, or any other keys to cancel.\n",
                &mut tcod.root);
            if let Some(inventory_index) = inventory_index {
                drop_item(inventory_index, game, objects);
            }
            DidntTakeTurn
        }
        _ => DidntTakeTurn,
    }
}

fn main() {
    let root = Root::initializer()
        .font("arial10x10.png", FontLayout::Tcod)
        .font_type(FontType::Greyscale)
        .size(SCREEN_WIDTH, SCREEN_HEIGHT)
        .title("Rust/libtcod tutorial")
        .init();

    tcod::system::set_fps(LIMIT_FPS);

    let mut tcod = Tcod {
        root: root,
        con: Offscreen::new(MAP_WIDTH, MAP_HEIGHT),
        panel: Offscreen::new(SCREEN_WIDTH, SCREEN_HEIGHT),
        fov: FovMap::new(MAP_WIDTH, MAP_HEIGHT),
        mouse: Default::default(),
    };

    // creation of objects
    let mut player = Object::new(0, 0, '@', "player", colors::WHITE, true);
    player.fighter = Some(Fighter{max_hp: 8, hp: 8, defense: 1, power: 2,
                                  on_death: DeathCallback::Player});
    player.alive = true;

    // objects list
    let mut objects = vec![player];

    let mut game = Game {
        map: make_map(&mut objects),
        log: vec![],
        inventory: vec![],
    };

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            tcod.fov.set(x, y,
                         !game.map[x as usize][y as usize].block_sight,
                         !game.map[x as usize][y as usize].blocked);
        }
    }

    let mut previous_player_position = (-1, -1);


    // welcome message
    message(&mut game.log, "Hello guys, let's play!", colors::RED);

    let mut key = Default::default();

    while !tcod.root.window_closed() {
        match input::check_for_event(input::MOUSE | input::KEY_PRESS) {
            Some((_, Event::Mouse(m))) => tcod.mouse = m,
            Some((_, Event::Key(k))) => key = k,
            _ => key = Default::default(),
        }

        let fov_recompute = previous_player_position != (objects[PLAYER].x,
                                                         objects[PLAYER].y);
        render_all(&mut tcod, &objects, &mut game, fov_recompute);

        tcod.root.flush();

        // eralse all objects from their old location, before moving
        for object in &objects {
            object.clear(&mut tcod.con)
        }

        // handle keys and exit the game if needed
        let player = &mut objects[PLAYER];
        previous_player_position = (player.x, player.y);
        let player_action = handle_keys(key, &mut tcod, &mut objects,
                                        &mut game);
        if player_action == PlayerAction::Exit {
            break
        }

        // let monsters take their turn
        if objects[PLAYER].alive && player_action != PlayerAction::DidntTakeTurn {
            for id in 0..objects.len() {
                if objects[id].ai.is_some() {
                    ai_take_turn(id, &mut game, &mut objects, &mut tcod.fov);
                }
            }
        }
    }
}
