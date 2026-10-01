"""Single source of truth for the Xianxia Cultivation add-on.

Everything here is consumed by build.py, which turns it into behavior-pack
JSON, resource-pack JSON, generated textures and `scripts/data.js` (the same
data, re-exported for the Script API runtime).
"""

NS = "xian"

# ---------------------------------------------------------------------------
# Cultivation realms
# ---------------------------------------------------------------------------
# stages: names of the minor stages inside a realm
# exp: cultivation base needed per minor stage (scaled by stage index at runtime)
# gain: base exp per second while meditating
# qi: maximum qi at the first stage of the realm
# chance: base breakthrough chance (%) INTO this realm
# tribulation: lightning waves to survive INTO this realm (0 = none)
# buffs: permanent effect amplifiers granted (-1 = none)
FOUR = ["Early", "Middle", "Late", "Peak"]
QC_LAYERS = [f"Layer {i}" for i in range(1, 10)]

REALMS = [
    dict(id="mortal", name="Mortal", color="§7", stages=["Mortal"], exp=40, gain=1, qi=20,
         chance=100, tribulation=0, lifespan=80,
         buffs=dict(health_boost=-1, strength=-1, resistance=-1, speed=-1, jump_boost=-1)),
    dict(id="qi_condensation", name="Qi Condensation", color="§f", stages=QC_LAYERS, exp=120, gain=2, qi=100,
         chance=100, tribulation=0, lifespan=120,
         buffs=dict(health_boost=0, strength=-1, resistance=-1, speed=-1, jump_boost=-1)),
    dict(id="foundation", name="Foundation Establishment", color="§a", stages=FOUR, exp=900, gain=6, qi=300,
         chance=60, tribulation=0, lifespan=200,
         buffs=dict(health_boost=1, strength=0, resistance=-1, speed=0, jump_boost=0)),
    dict(id="core_formation", name="Core Formation", color="§e", stages=FOUR, exp=3000, gain=15, qi=800,
         chance=45, tribulation=3, lifespan=500,
         buffs=dict(health_boost=3, strength=1, resistance=0, speed=0, jump_boost=0)),
    dict(id="nascent_soul", name="Nascent Soul", color="§6", stages=FOUR, exp=9000, gain=38, qi=2000,
         chance=35, tribulation=5, lifespan=1000,
         buffs=dict(health_boost=5, strength=2, resistance=0, speed=1, jump_boost=1)),
    dict(id="soul_transformation", name="Soul Transformation", color="§b", stages=FOUR, exp=26000, gain=95, qi=5000,
         chance=30, tribulation=6, lifespan=2000,
         buffs=dict(health_boost=7, strength=3, resistance=1, speed=1, jump_boost=1)),
    dict(id="void_refinement", name="Void Refinement", color="§3", stages=FOUR, exp=75000, gain=240, qi=12000,
         chance=25, tribulation=7, lifespan=5000,
         buffs=dict(health_boost=9, strength=4, resistance=1, speed=1, jump_boost=1)),
    dict(id="body_integration", name="Body Integration", color="§9", stages=FOUR, exp=210000, gain=600, qi=30000,
         chance=22, tribulation=8, lifespan=10000,
         buffs=dict(health_boost=12, strength=5, resistance=2, speed=2, jump_boost=2)),
    dict(id="mahayana", name="Mahayana", color="§d", stages=FOUR, exp=600000, gain=1500, qi=70000,
         chance=18, tribulation=9, lifespan=30000,
         buffs=dict(health_boost=15, strength=6, resistance=2, speed=2, jump_boost=2)),
    dict(id="tribulation_transcendence", name="Tribulation Transcendence", color="§5", stages=FOUR, exp=1700000,
         gain=3800, qi=160000, chance=14, tribulation=12, lifespan=100000,
         buffs=dict(health_boost=19, strength=7, resistance=3, speed=2, jump_boost=2)),
    dict(id="true_immortal", name="True Immortal", color="§c§l", stages=["Ascended"], exp=10 ** 9, gain=9000,
         qi=400000, chance=10, tribulation=18, lifespan=-1,
         buffs=dict(health_boost=24, strength=8, resistance=3, speed=3, jump_boost=3)),
]

# ---------------------------------------------------------------------------
# Body cultivation (separate track, levelled by taking damage + pills)
# ---------------------------------------------------------------------------
BODY_TIERS = [
    dict(name="Mortal Body", exp=0, health=-1, resistance=-1),
    dict(name="Copper Skin", exp=150, health=0, resistance=-1),
    dict(name="Iron Bone", exp=600, health=1, resistance=-1),
    dict(name="Silver Marrow", exp=2000, health=2, resistance=0),
    dict(name="Jade Flesh", exp=6000, health=3, resistance=0),
    dict(name="Golden Body", exp=18000, health=4, resistance=1),
    dict(name="Vajra Diamond Body", exp=50000, health=6, resistance=1),
    dict(name="Immortal Primordial Body", exp=150000, health=8, resistance=2),
]

# ---------------------------------------------------------------------------
# Spirit roots
# ---------------------------------------------------------------------------
ELEMENTS = {
    "metal": dict(name="Metal", color="§f", rgb=(0.92, 0.88, 0.70)),
    "wood": dict(name="Wood", color="§a", rgb=(0.35, 0.85, 0.35)),
    "water": dict(name="Water", color="§9", rgb=(0.30, 0.55, 1.00)),
    "fire": dict(name="Fire", color="§c", rgb=(1.00, 0.40, 0.15)),
    "earth": dict(name="Earth", color="§6", rgb=(0.75, 0.55, 0.25)),
    "lightning": dict(name="Lightning", color="§d", rgb=(0.75, 0.55, 1.00), mutated=True),
    "ice": dict(name="Ice", color="§b", rgb=(0.65, 0.95, 1.00), mutated=True),
    "wind": dict(name="Wind", color="§2", rgb=(0.70, 1.00, 0.75), mutated=True),
    "none": dict(name="Neutral", color="§7", rgb=(0.55, 0.80, 1.00)),
}

# grade by number of basic elements (or mutated)
ROOT_GRADES = {
    "heavenly": dict(name="Heavenly Spirit Root", mult=3.0, color="§6§l"),
    "mutated": dict(name="Mutated Spirit Root", mult=2.6, color="§d§l"),
    "dual": dict(name="Earth Spirit Root (Dual)", mult=2.0, color="§e"),
    "triple": dict(name="Human Spirit Root (Triple)", mult=1.4, color="§a"),
    "quad": dict(name="Mixed Spirit Root (Quad)", mult=1.0, color="§f"),
    "pseudo": dict(name="Pseudo Spirit Root (Five)", mult=0.6, color="§7"),
}

# ---------------------------------------------------------------------------
# Cultivation manuals (jade slips): passive cultivation-speed arts
# ---------------------------------------------------------------------------
MANUALS = [
    dict(id="basic_breathing", name="Basic Qi Breathing Technique", grade="Mortal", element="none", mult=1.0,
         desc="The breathing method every mortal sect hands out. Slow, but stable."),
    dict(id="five_elements", name="Five Elements Harmony Sutra", grade="Yellow", element="none", mult=1.2,
         desc="+0.15x for every element in your spirit root. Beloved by the mixed-root."),
    dict(id="azure_wood", name="Azure Wood Everlasting Art", grade="Profound", element="wood", mult=1.5,
         desc="Wood qi nourishes life. Slowly heals you while meditating."),
    dict(id="blazing_sun", name="Blazing Sun Scripture", grade="Profound", element="fire", mult=1.5,
         desc="Twice as effective under the noon sun."),
    dict(id="profound_water", name="Profound Water Mirror Canon", grade="Profound", element="water", mult=1.5,
         desc="Meditating in or beside water strengthens the art."),
    dict(id="golden_vajra", name="Indestructible Golden Vajra Art", grade="Profound", element="metal", mult=1.4,
         desc="Also tempers the body while you cultivate."),
    dict(id="great_earth", name="Great Earth Mountain Art", grade="Profound", element="earth", mult=1.5,
         desc="Stronger the deeper underground you meditate."),
    dict(id="nine_heavens_thunder", name="Nine Heavens Thunder Tribulation Art", grade="Earth", element="lightning",
         mult=2.0, desc="Triple speed during thunderstorms. Halves tribulation damage."),
    dict(id="frost_moon", name="Frost Moon Yin Sutra", grade="Earth", element="ice", mult=2.0,
         desc="A yin art: +50% at night, doubled under a full moon."),
    dict(id="void_wind", name="Boundless Void Wind Manual", grade="Earth", element="wind", mult=2.0,
         desc="Stronger at high altitude. Grants lighter steps."),
    dict(id="heavenly_devouring", name="Heavenly Devouring Demon Art", grade="Earth", element="none", mult=1.8,
         desc="DEMONIC. Devour the essence of the slain: 5x cultivation from kills. Risk of qi deviation."),
    dict(id="primordial_chaos", name="Primordial Chaos Scripture", grade="Heaven", element="none", mult=3.0,
         desc="An art from before Heaven and Earth split. Every element counts as compatible."),
]

GRADE_COLORS = {"Mortal": "§7", "Yellow": "§e", "Profound": "§b", "Earth": "§6", "Heaven": "§d"}

# ---------------------------------------------------------------------------
# Martial techniques (learned from technique scrolls, cast with the Martial Seal)
# ---------------------------------------------------------------------------
# realm: minimum realm index, cost: qi, cd: cooldown ticks, power: base damage
TECHNIQUES = [
    dict(id="qi_bolt", name="Spirit Qi Bolt", element="none", realm=1, cost=8, cd=10, power=5,
         desc="Condense qi into a bolt and fire it."),
    dict(id="wind_step", name="Wind Step", element="wind", realm=1, cost=10, cd=20, power=0,
         desc="Dash forward on the wind."),
    dict(id="fireball", name="Blazing Fireball", element="fire", realm=1, cost=18, cd=20, power=7,
         desc="A ball of spirit fire that bursts and ignites."),
    dict(id="sword_qi", name="Crescent Sword Qi", element="metal", realm=1, cost=18, cd=16, power=6,
         desc="A crescent of sword intent that pierces through enemies."),
    dict(id="frost_spikes", name="Frost Lotus Bloom", element="ice", realm=1, cost=25, cd=40, power=5,
         desc="Frost erupts around you, freezing all nearby foes."),
    dict(id="healing_spring", name="Verdant Healing Spring", element="wood", realm=1, cost=30, cd=200, power=0,
         desc="Wood qi heals you and nearby allies."),
    dict(id="spirit_sense", name="Divine Sense Sweep", element="none", realm=1, cost=5, cd=60, power=0,
         desc="Sweep the area with your spiritual sense and reveal every living thing."),
    dict(id="vine_bind", name="Entangling Spirit Vines", element="wood", realm=2, cost=25, cd=60, power=4,
         desc="Vines bind the target in place."),
    dict(id="tidal_palm", name="Surging Tide Palm", element="water", realm=2, cost=30, cd=40, power=6,
         desc="A wave of water qi crashes forward, hurling enemies back."),
    dict(id="earth_shield", name="Mountain Guardian Stance", element="earth", realm=2, cost=40, cd=400, power=0,
         desc="Root yourself like a mountain: heavy damage reduction and absorption."),
    dict(id="golden_bell", name="Golden Bell Shield", element="metal", realm=2, cost=45, cd=500, power=0,
         desc="A golden bell of qi makes you nearly invulnerable for a short time."),
    dict(id="thunder_palm", name="Thunder Palm", element="lightning", realm=2, cost=35, cd=40, power=10,
         desc="Call down a bolt of lightning on your target."),
    dict(id="phoenix_flame", name="Vermilion Bird Inferno", element="fire", realm=3, cost=80, cd=100, power=9,
         desc="A cone of phoenix fire incinerates everything in front of you."),
    dict(id="buddha_palm", name="Tathagata Buddha Palm", element="earth", realm=3, cost=120, cd=160, power=16,
         desc="A colossal palm descends from the heavens onto your target."),
    dict(id="heavenly_thunder", name="Heavenly Thunder Calamity", element="lightning", realm=3, cost=120, cd=200,
         power=12, desc="Thunder rains down on every enemy around you."),
    dict(id="void_step", name="Void Step", element="none", realm=4, cost=50, cd=40, power=0,
         desc="Step through the void to where you are looking."),
    dict(id="sword_rain", name="Ten Thousand Swords Return", element="metal", realm=4, cost=200, cd=240, power=10,
         desc="A rain of qi swords falls across the battlefield."),
    dict(id="blood_sacrifice", name="Blood Demon Sacrifice", element="none", realm=2, cost=0, cd=120, power=18,
         desc="DEMONIC. Burn your own blood for a devastating, life-stealing strike."),
    dict(id="heavenly_domain", name="Heavenly Domain", element="none", realm=5, cost=400, cd=600, power=6,
         desc="Unfold your domain: enemies inside are suppressed and ground down."),
    dict(id="starfall", name="Starfall Annihilation", element="none", realm=8, cost=1500, cd=900, power=40,
         desc="Pull stars from the sky onto your foes."),
]

# ---------------------------------------------------------------------------
# Herbs (plantable crops that also generate in the world)
# ---------------------------------------------------------------------------
HERBS = [
    dict(id="spirit_grass", name="Spirit Grass", colors=((70, 200, 110), (150, 255, 190)), style="grass",
         biomes=["plains", "forest", "meadow", "birch"], rarity=4),
    dict(id="blood_ginseng", name="Blood Ginseng", colors=((60, 120, 50), (200, 30, 40)), style="berry",
         biomes=["forest", "taiga", "roofed"], rarity=10),
    dict(id="frost_lotus", name="Frost Lotus", colors=((120, 200, 230), (235, 250, 255)), style="lotus",
         biomes=["frozen", "cold", "ice", "mountains"], rarity=8),
    dict(id="flame_lotus", name="Flame Lotus", colors=((120, 70, 40), (255, 120, 30)), style="lotus",
         biomes=["desert", "mesa", "savanna", "nether"], rarity=8),
    dict(id="purple_cloud_mushroom", name="Purple Cloud Mushroom", colors=((220, 210, 190), (150, 70, 210)),
         style="mushroom", biomes=["roofed", "swamp", "mushroom_island", "lush_caves"], rarity=8),
    dict(id="moon_orchid", name="Moon Orchid", colors=((70, 150, 90), (225, 225, 255)), style="flower",
         biomes=["flower_forest", "cherry_grove", "meadow", "forest"], rarity=9),
    dict(id="thunder_fern", name="Thunderstrike Fern", colors=((50, 110, 90), (200, 140, 255)), style="fern",
         biomes=["jungle", "mountains", "extreme_hills", "stony_peaks"], rarity=10),
    dict(id="jade_dew_flower", name="Jade Dew Flower", colors=((60, 160, 120), (120, 255, 200)), style="flower",
         biomes=["river", "beach", "swamp", "mangrove_swamp"], rarity=7),
]

# ---------------------------------------------------------------------------
# Generic materials
# ---------------------------------------------------------------------------
MATERIALS = [
    dict(id="low_spirit_stone", name="Low-Grade Spirit Stone", art="stone", color=(110, 200, 255)),
    dict(id="mid_spirit_stone", name="Mid-Grade Spirit Stone", art="stone", color=(90, 255, 160)),
    dict(id="high_spirit_stone", name="High-Grade Spirit Stone", art="stone", color=(255, 210, 80)),
    dict(id="supreme_spirit_stone", name="Supreme Spirit Stone", art="stone", color=(240, 110, 255)),
    dict(id="spirit_jade", name="Spirit Jade", art="jade", color=(80, 220, 140)),
    dict(id="raw_profound_iron", name="Raw Profound Iron", art="raw", color=(80, 90, 130)),
    dict(id="profound_iron_ingot", name="Profound Iron Ingot", art="ingot", color=(100, 120, 190)),
    dict(id="low_beast_core", name="Low-Grade Beast Core", art="core", color=(200, 80, 60)),
    dict(id="mid_beast_core", name="Mid-Grade Beast Core", art="core", color=(230, 150, 40)),
    dict(id="high_beast_core", name="High-Grade Beast Core", art="core", color=(180, 60, 255)),
    dict(id="king_beast_core", name="Beast King Core", art="core", color=(255, 230, 90)),
    dict(id="demon_core", name="Demonic Core", art="core", color=(120, 0, 30)),
    dict(id="dragon_scale", name="Flood Dragon Scale", art="scale", color=(60, 170, 200)),
    dict(id="tribulation_essence", name="Tribulation Lightning Essence", art="essence", color=(200, 160, 255)),
    dict(id="fox_spirit_tail", name="Spirit Fox Tail", art="tail", color=(240, 240, 255)),
    dict(id="tiger_bone", name="Flame Tiger Bone", art="bone", color=(240, 220, 190)),
    dict(id="wolf_fang", name="Demonic Wolf Fang", art="fang", color=(230, 230, 220)),
    dict(id="talisman_paper", name="Blank Talisman Paper", art="paper", color=(235, 200, 90)),
    dict(id="cinnabar_ink", name="Cinnabar Spirit Ink", art="ink", color=(210, 30, 30)),
    dict(id="pill_residue", name="Pill Residue", art="residue", color=(70, 60, 50)),
    dict(id="spirit_fruit", name="Spirit Fruit", art="fruit", color=(255, 120, 150)),
    dict(id="immortal_peach", name="Immortal Peach", art="peach", color=(255, 170, 160)),
]

# Edible materials (handled by script on completion)
EDIBLE = {"spirit_fruit", "immortal_peach"}

# ---------------------------------------------------------------------------
# Pills. ingredients are item ids without namespace -> count
# ---------------------------------------------------------------------------
PILLS = [
    dict(id="qi_gathering_pill", name="Qi Gathering Pill", color=(120, 200, 255), level=1, toxicity=8,
         ingredients={"spirit_grass": 2, "low_spirit_stone": 1}, chance=85,
         desc="Grants cultivation equal to ~40s of meditation."),
    dict(id="healing_pill", name="Healing Pill", color=(255, 120, 120), level=1, toxicity=4,
         ingredients={"spirit_grass": 1, "blood_ginseng": 1}, chance=85,
         desc="Instantly heals and grants regeneration."),
    dict(id="bigu_pill", name="Bigu Pill", color=(230, 210, 150), level=1, toxicity=0,
         ingredients={"spirit_grass": 2}, chance=90,
         desc="An inedia pill. Fills your stomach for days."),
    dict(id="qi_recovery_pill", name="Qi Recovery Pill", color=(150, 170, 255), level=1, toxicity=5,
         ingredients={"moon_orchid": 1, "jade_dew_flower": 1}, chance=80,
         desc="Restores half of your qi."),
    dict(id="detox_pill", name="Purifying Detox Pill", color=(170, 255, 200), level=2, toxicity=0,
         ingredients={"jade_dew_flower": 2, "spirit_grass": 2}, chance=75,
         desc="Purges pill toxicity and poison."),
    dict(id="spirit_condensing_pill", name="Spirit Condensing Pill", color=(90, 255, 220), level=2, toxicity=12,
         ingredients={"spirit_grass": 2, "moon_orchid": 1, "jade_dew_flower": 1, "low_spirit_stone": 2},
         chance=70, desc="Grants cultivation equal to ~2 minutes of meditation."),
    dict(id="body_tempering_pill", name="Body Tempering Pill", color=(200, 90, 60), level=2, toxicity=10,
         ingredients={"blood_ginseng": 2, "low_beast_core": 1}, chance=70,
         desc="Tempers flesh and bone (+body cultivation)."),
    dict(id="berserk_blood_pill", name="Berserk Blood Pill", color=(170, 20, 30), level=2, toxicity=15,
         ingredients={"blood_ginseng": 2, "flame_lotus": 1}, chance=70,
         desc="Burn your blood: great strength and speed, then weakness."),
    dict(id="foundation_pill", name="Foundation Establishment Pill", color=(120, 255, 120), level=3, toxicity=20,
         ingredients={"blood_ginseng": 2, "jade_dew_flower": 2, "moon_orchid": 1, "mid_spirit_stone": 1,
                      "low_beast_core": 1}, chance=55,
         desc="+30% breakthrough chance (any realm, best for Foundation)."),
    dict(id="clear_heart_pill", name="Clear Heart Pill", color=(220, 240, 255), level=3, toxicity=5,
         ingredients={"moon_orchid": 2, "purple_cloud_mushroom": 1}, chance=65,
         desc="Cures qi deviation and steadies your dao heart."),
    dict(id="enlightenment_pill", name="Dao Enlightenment Pill", color=(255, 250, 180), level=4, toxicity=15,
         ingredients={"thunder_fern": 1, "moon_orchid": 1, "spirit_jade": 1}, chance=50,
         desc="+5 Dao Comprehension (each point: +0.5% breakthrough chance)."),
    dict(id="golden_core_pill", name="Golden Core Pill", color=(255, 210, 60), level=4, toxicity=25,
         ingredients={"flame_lotus": 2, "frost_lotus": 2, "mid_beast_core": 1, "mid_spirit_stone": 2}, chance=45,
         desc="+30% breakthrough chance (any realm, best for Core Formation)."),
    dict(id="bone_marrow_pill", name="Bone Marrow Cleansing Pill", color=(255, 255, 255), level=5, toxicity=30,
         ingredients={"frost_lotus": 1, "flame_lotus": 1, "thunder_fern": 1, "blood_ginseng": 1,
                      "high_spirit_stone": 1}, chance=40,
         desc="Washes the marrow and refines your spirit root to a higher grade."),
    dict(id="nascent_soul_pill", name="Nascent Soul Pill", color=(255, 160, 60), level=5, toxicity=30,
         ingredients={"purple_cloud_mushroom": 3, "thunder_fern": 2, "high_beast_core": 1,
                      "high_spirit_stone": 1}, chance=40,
         desc="+30% breakthrough chance (any realm, best for Nascent Soul)."),
    dict(id="longevity_pill", name="Longevity Pill", color=(255, 190, 200), level=6, toxicity=20,
         ingredients={"immortal_peach": 1, "spirit_jade": 2, "high_spirit_stone": 1}, chance=40,
         desc="+lifespan, full heal and a large boost of cultivation."),
    dict(id="void_tribulation_pill", name="Void Tribulation Pill", color=(190, 120, 255), level=7, toxicity=35,
         ingredients={"tribulation_essence": 1, "dragon_scale": 2, "king_beast_core": 1,
                      "supreme_spirit_stone": 1}, chance=35,
         desc="+25% breakthrough chance and halves heavenly tribulation damage."),
]
# Breakthrough pill bonuses: pill id -> (best realm index, bonus %)
BREAKTHROUGH_PILLS = {
    "foundation_pill": (2, 30),
    "golden_core_pill": (3, 30),
    "nascent_soul_pill": (4, 30),
    "void_tribulation_pill": (-1, 25),
}

# ---------------------------------------------------------------------------
# Talismans (single-use)
# ---------------------------------------------------------------------------
TALISMANS = [
    dict(id="fire_talisman", name="Flame Burst Talisman", rune=(220, 40, 20),
         recipe=["talisman_paper", "cinnabar_ink", "minecraft:blaze_powder"], desc="Hurls a fireball."),
    dict(id="thunder_talisman", name="Five Thunder Talisman", rune=(150, 60, 220),
         recipe=["talisman_paper", "cinnabar_ink", "thunder_fern"], desc="Calls lightning on your target."),
    dict(id="protection_talisman", name="Golden Light Protection Talisman", rune=(220, 170, 20),
         recipe=["talisman_paper", "cinnabar_ink", "minecraft:iron_ingot"], desc="Absorption and resistance."),
    dict(id="divine_travel_talisman", name="Divine Travel Talisman", rune=(40, 160, 70),
         recipe=["talisman_paper", "cinnabar_ink", "minecraft:feather"], desc="Run like the wind for a minute."),
    dict(id="escape_talisman", name="Thousand Mile Escape Talisman", rune=(40, 90, 210),
         recipe=["talisman_paper", "cinnabar_ink", "minecraft:ender_pearl"],
         desc="Instantly flee to your spawn point."),
    dict(id="concealment_talisman", name="Concealment Talisman", rune=(90, 90, 110),
         recipe=["talisman_paper", "cinnabar_ink", "minecraft:fermented_spider_eye"], desc="Become invisible."),
    dict(id="sealing_talisman", name="Immortal Binding Seal", rune=(160, 20, 60),
         recipe=["talisman_paper", "cinnabar_ink", "minecraft:chain"], desc="Seals your target in place."),
    dict(id="lightning_rod_talisman", name="Tribulation Warding Talisman", rune=(250, 230, 120),
         recipe=["talisman_paper", "cinnabar_ink", "tribulation_essence"],
         desc="Consumed during a heavenly tribulation to halve its damage."),
]

# ---------------------------------------------------------------------------
# Weapons / tools / armor
# ---------------------------------------------------------------------------
WEAPONS = [
    dict(id="spirit_iron_sword", name="Spirit Iron Sword", damage=8, durability=900, blade=(170, 190, 230),
         hilt=(90, 60, 40), gem=(110, 200, 255), effect=None),
    dict(id="azure_frost_sword", name="Azure Frost Sword", damage=10, durability=1400, blade=(170, 235, 255),
         hilt=(60, 90, 160), gem=(220, 250, 255), effect="frost"),
    dict(id="crimson_flame_saber", name="Crimson Flame Saber", damage=11, durability=1400, blade=(255, 120, 60),
         hilt=(110, 30, 20), gem=(255, 220, 80), effect="flame", style="saber"),
    dict(id="thunderclap_spear", name="Thunderclap Spear", damage=12, durability=1600, blade=(210, 190, 255),
         hilt=(70, 50, 90), gem=(160, 90, 255), effect="thunder", style="spear"),
    dict(id="blood_demon_blade", name="Blood Demon Blade", damage=13, durability=1600, blade=(150, 10, 25),
         hilt=(30, 10, 10), gem=(255, 40, 60), effect="lifesteal", style="saber"),
    dict(id="heaven_severing_sword", name="Heaven Severing Sword", damage=18, durability=4000,
         blade=(255, 245, 210), hilt=(200, 160, 40), gem=(255, 90, 220), effect="sever"),
    dict(id="flying_sword", name="Flying Sword", damage=7, durability=1200, blade=(140, 230, 255),
         hilt=(40, 120, 120), gem=(140, 255, 210), effect=None, flying=True),
]

ARMOR_SETS = [
    dict(id="daoist", name="Daoist", base=(70, 120, 200), trim=(240, 240, 250), protection=(3, 7, 5, 2),
         durability=420, pieces=("Jade Crown", "Daoist Robe", "Daoist Trousers", "Cloud-Treading Boots")),
    dict(id="celestial", name="Celestial", base=(245, 240, 225), trim=(230, 180, 50), protection=(4, 9, 7, 4),
         durability=1100, pieces=("Celestial Crown", "Celestial Immortal Robe", "Celestial Leggings",
                                  "Celestial Cloud Boots")),
]

# ---------------------------------------------------------------------------
# Special usable items
# ---------------------------------------------------------------------------
SPECIAL_ITEMS = [
    dict(id="cultivation_codex", name="Cultivation Codex", art="codex", stack=1,
         desc="Your cultivation status, breakthroughs, sect, guide and settings."),
    dict(id="martial_seal", name="Martial Jade Seal", art="seal", stack=1,
         desc="Use to cast your selected technique. Sneak + use to choose a technique."),
    dict(id="root_testing_stone", name="Spirit Root Testing Stone", art="orb", stack=16,
         desc="Reveals your spirit root."),
    dict(id="dragon_summoning_pearl", name="Dragon Summoning Pearl", art="pearl", stack=1,
         desc="Use near water to summon the Azure Flood Dragon."),
    dict(id="beast_tide_horn", name="Beast Tide Horn", art="horn", stack=1,
         desc="Provoke a beast tide around you. Great rewards, great danger."),
    dict(id="sect_token", name="Sect Token", art="token", stack=1,
         desc="Opens your sect's mission hall and treasury."),
    dict(id="spatial_ring", name="Spatial Storage Ring", art="ring", stack=1,
         desc="A pocket dimension: store and retrieve items anywhere."),
]

# ---------------------------------------------------------------------------
# Blocks
# ---------------------------------------------------------------------------
ORES = [
    dict(id="spirit_stone_ore", name="Spirit Stone Ore", host="stone", gem=(110, 200, 255), drop="low_spirit_stone",
         count=(1, 2), y=(-30, 70), size=6, iterations=10, replace=["minecraft:stone", "minecraft:andesite",
                                                                     "minecraft:diorite", "minecraft:granite"],
         light=3),
    dict(id="deepslate_spirit_stone_ore", name="Deepslate Spirit Stone Ore", host="deepslate", gem=(90, 255, 160),
         drop="low_spirit_stone", count=(2, 4), y=(-64, 0), size=6, iterations=8, replace=["minecraft:deepslate"],
         light=4, bonus="mid_spirit_stone"),
    dict(id="spirit_jade_ore", name="Spirit Jade Ore", host="stone", gem=(80, 220, 140), drop="spirit_jade",
         count=(1, 1), y=(-40, 40), size=4, iterations=3, replace=["minecraft:stone", "minecraft:deepslate"],
         light=5),
    dict(id="profound_iron_ore", name="Profound Iron Ore", host="deepslate", gem=(110, 130, 210),
         drop="raw_profound_iron", count=(1, 2), y=(-64, 16), size=5, iterations=5,
         replace=["minecraft:deepslate", "minecraft:stone"], light=0),
]

# Functional blocks; kind decides geometry / behaviour
FUNCTION_BLOCKS = [
    dict(id="alchemy_furnace", name="Alchemy Furnace", kind="furnace", light=10),
    dict(id="refining_forge", name="Artifact Refining Forge", kind="forge", light=8),
    dict(id="meditation_cushion", name="Meditation Cushion", kind="cushion", light=0),
    dict(id="qi_gathering_array", name="Qi Gathering Array", kind="array", light=8, rune=(120, 220, 255)),
    dict(id="protection_array", name="Heaven Guarding Array", kind="array", light=8, rune=(255, 210, 90)),
    dict(id="teleport_array", name="Teleportation Array", kind="array", light=10, rune=(200, 120, 255)),
    dict(id="spirit_vein", name="Spirit Vein", kind="vein", light=12),
    dict(id="spirit_stone_block", name="Block of Spirit Stone", kind="storage", light=6),
]

# ---------------------------------------------------------------------------
# Refining (forge) recipes: output id -> ingredients
# ---------------------------------------------------------------------------
REFINING = [
    dict(out="spirit_iron_sword", level=1, chance=85,
         ingredients={"profound_iron_ingot": 3, "low_spirit_stone": 4, "minecraft:stick": 1}),
    dict(out="flying_sword", level=2, chance=75,
         ingredients={"profound_iron_ingot": 4, "spirit_jade": 1, "mid_spirit_stone": 2, "minecraft:feather": 4}),
    dict(out="azure_frost_sword", level=3, chance=65,
         ingredients={"profound_iron_ingot": 4, "frost_lotus": 3, "mid_beast_core": 1, "mid_spirit_stone": 3}),
    dict(out="crimson_flame_saber", level=3, chance=65,
         ingredients={"profound_iron_ingot": 4, "flame_lotus": 3, "tiger_bone": 2, "mid_spirit_stone": 3}),
    dict(out="thunderclap_spear", level=4, chance=55,
         ingredients={"profound_iron_ingot": 5, "thunder_fern": 3, "high_beast_core": 1, "mid_spirit_stone": 4}),
    dict(out="blood_demon_blade", level=4, chance=55,
         ingredients={"profound_iron_ingot": 5, "demon_core": 2, "blood_ginseng": 3, "mid_spirit_stone": 4}),
    dict(out="heaven_severing_sword", level=7, chance=35,
         ingredients={"profound_iron_ingot": 8, "tribulation_essence": 2, "dragon_scale": 4, "king_beast_core": 1,
                      "supreme_spirit_stone": 2}),
    dict(out="daoist_helmet", level=1, chance=85, ingredients={"spirit_jade": 2, "minecraft:white_wool": 3}),
    dict(out="daoist_chestplate", level=1, chance=85,
         ingredients={"minecraft:blue_wool": 6, "minecraft:white_wool": 2, "low_spirit_stone": 4}),
    dict(out="daoist_leggings", level=1, chance=85,
         ingredients={"minecraft:blue_wool": 5, "low_spirit_stone": 3}),
    dict(out="daoist_boots", level=1, chance=85, ingredients={"minecraft:white_wool": 2, "minecraft:feather": 2,
                                                              "low_spirit_stone": 2}),
    dict(out="celestial_helmet", level=5, chance=50,
         ingredients={"daoist_helmet": 1, "spirit_jade": 3, "high_spirit_stone": 1, "minecraft:gold_ingot": 4}),
    dict(out="celestial_chestplate", level=5, chance=50,
         ingredients={"daoist_chestplate": 1, "dragon_scale": 2, "high_spirit_stone": 2,
                      "minecraft:gold_ingot": 6}),
    dict(out="celestial_leggings", level=5, chance=50,
         ingredients={"daoist_leggings": 1, "fox_spirit_tail": 2, "high_spirit_stone": 2,
                      "minecraft:gold_ingot": 5}),
    dict(out="celestial_boots", level=5, chance=50,
         ingredients={"daoist_boots": 1, "minecraft:feather": 6, "high_spirit_stone": 1,
                      "minecraft:gold_ingot": 3}),
    dict(out="qi_gathering_array", level=2, chance=75,
         ingredients={"spirit_jade": 2, "low_spirit_stone": 8, "minecraft:smooth_stone_slab": 1}),
    dict(out="protection_array", level=3, chance=65,
         ingredients={"spirit_jade": 2, "mid_spirit_stone": 2, "minecraft:iron_block": 1,
                      "minecraft:smooth_stone_slab": 1}),
    dict(out="teleport_array", level=3, chance=65,
         ingredients={"spirit_jade": 3, "mid_spirit_stone": 2, "minecraft:ender_pearl": 4,
                      "minecraft:smooth_stone_slab": 1}),
    dict(out="spatial_ring", level=4, chance=55,
         ingredients={"spirit_jade": 2, "minecraft:gold_ingot": 2, "minecraft:ender_chest": 1,
                      "mid_spirit_stone": 3}),
    dict(out="dragon_summoning_pearl", level=5, chance=50,
         ingredients={"king_beast_core": 1, "minecraft:heart_of_the_sea": 1, "high_spirit_stone": 2}),
    dict(out="beast_tide_horn", level=2, chance=75,
         ingredients={"minecraft:goat_horn": 1, "wolf_fang": 3, "low_beast_core": 2}),
]

# ---------------------------------------------------------------------------
# Crafting-table recipes (shaped/shapeless) for the entry-level items
# ---------------------------------------------------------------------------
SHAPED = [
    dict(id="alchemy_furnace", pattern=["CIC", "I I", "SBS"],
         key={"C": "minecraft:copper_ingot", "I": "minecraft:iron_ingot", "S": "low_spirit_stone",
              "B": "minecraft:blast_furnace"}, count=1),
    dict(id="refining_forge", pattern=["III", "SAS", "OOO"],
         key={"I": "minecraft:iron_ingot", "S": "low_spirit_stone", "A": "minecraft:anvil",
              "O": "minecraft:obsidian"}, count=1),
    dict(id="meditation_cushion", pattern=["WWW", "SCS"],
         key={"W": "minecraft:yellow_wool", "S": "low_spirit_stone", "C": "minecraft:white_carpet"}, count=1),
    dict(id="cultivation_codex", pattern=["S", "B"],
         key={"S": "low_spirit_stone", "B": "minecraft:book"}, count=1),
    dict(id="martial_seal", pattern=[" J ", "JSJ", " J "],
         key={"J": "spirit_jade", "S": "low_spirit_stone"}, count=1),
    dict(id="root_testing_stone", pattern=[" G ", "GSG", " G "],
         key={"G": "minecraft:glass", "S": "low_spirit_stone"}, count=1),
    dict(id="spirit_stone_block", pattern=["SSS", "SSS", "SSS"], key={"S": "low_spirit_stone"}, count=1),
    dict(id="talisman_paper", pattern=["PPP", "PGP", "PPP"],
         key={"P": "minecraft:paper", "G": "spirit_grass"}, count=8),
    dict(id="sect_token", pattern=[" J ", "JGJ", " J "],
         key={"J": "minecraft:gold_nugget", "G": "spirit_jade"}, count=1),
    dict(id="manual_basic_breathing", pattern=["PPP", "PSP", "PPP"],
         key={"P": "minecraft:paper", "S": "low_spirit_stone"}, count=1),
    dict(id="scroll_qi_bolt", pattern=["PIP", "PSP"],
         key={"P": "minecraft:paper", "I": "cinnabar_ink", "S": "low_spirit_stone"}, count=1),
]
SHAPELESS = [
    dict(id="cinnabar_ink", ingredients=["minecraft:redstone", "minecraft:redstone", "minecraft:glass_bottle",
                                         "low_spirit_stone"], count=2),
    dict(id="profound_iron_ingot", ingredients=["raw_profound_iron", "raw_profound_iron", "minecraft:iron_ingot",
                                                "minecraft:coal"], count=1),
    dict(id="mid_spirit_stone", ingredients=["low_spirit_stone"] * 9, count=1),
    dict(id="high_spirit_stone", ingredients=["mid_spirit_stone"] * 9, count=1),
    dict(id="supreme_spirit_stone", ingredients=["high_spirit_stone"] * 9, count=1),
    dict(id="low_spirit_stone", ingredients=["mid_spirit_stone"], count=9, tag="unpack_mid"),
    dict(id="mid_spirit_stone", ingredients=["high_spirit_stone"], count=9, tag="unpack_high"),
    dict(id="high_spirit_stone", ingredients=["supreme_spirit_stone"], count=9, tag="unpack_supreme"),
    dict(id="low_spirit_stone", ingredients=["spirit_stone_block"], count=9, tag="unpack_block"),
]
# Talisman shapeless recipes are generated from TALISMANS[].recipe

# Furnace smelting
SMELTING = [dict(input="raw_profound_iron", output="profound_iron_ingot")]

# ---------------------------------------------------------------------------
# Sects
# ---------------------------------------------------------------------------
SECTS = [
    dict(id="azure_cloud", name="Azure Cloud Sword Sect", alignment="righteous", color="§b",
         perk="Martial techniques deal +20% damage. Sword techniques cost 20% less qi."),
    dict(id="myriad_pill", name="Myriad Pill Valley", alignment="righteous", color="§a",
         perk="+15% alchemy and refining success, +1 pill per successful batch."),
    dict(id="heavenly_secrets", name="Heavenly Secrets Pavilion", alignment="neutral", color="§e",
         perk="+15% cultivation speed and +5% breakthrough chance."),
    dict(id="blood_moon", name="Blood Moon Demonic Sect", alignment="demonic", color="§4",
         perk="Double cultivation from kills, 10% lifesteal. Karma no longer worsens tribulations."),
]
SECT_RANKS = [
    dict(name="Outer Disciple", contribution=0),
    dict(name="Inner Disciple", contribution=300),
    dict(name="Core Disciple", contribution=1500),
    dict(name="Elder", contribution=6000),
    dict(name="Grand Elder", contribution=20000),
    dict(name="Sect Master", contribution=60000),
]
SECT_MISSIONS = [
    dict(id="cull_beasts", name="Cull the Beast Horde", kind="kill_hostile", target=15, reward=60, stones=6,
         desc="Slay 15 hostile creatures."),
    dict(id="hunt_spirit_beasts", name="Spirit Beast Hunt", kind="kill_beast", target=4, reward=90, stones=10,
         desc="Slay 4 spirit beasts (wolves, tigers)."),
    dict(id="slay_demons", name="Purge the Demonic Path", kind="kill_demonic", target=2, reward=150, stones=16,
         desc="Slay 2 demonic or rogue cultivators.", alignment="righteous"),
    dict(id="harvest_tribute", name="Righteous Harvest Tribute", kind="kill_villager", target=3, reward=150,
         stones=16, desc="Offer 3 mortal souls to the Blood Moon.", alignment="demonic"),
    dict(id="deliver_herbs", name="Herb Tribute", kind="deliver", item="spirit_grass", target=12, reward=50,
         stones=4, desc="Deliver 12 Spirit Grass."),
    dict(id="deliver_ginseng", name="Blood Ginseng Request", kind="deliver", item="blood_ginseng", target=6,
         reward=80, stones=8, desc="Deliver 6 Blood Ginseng."),
    dict(id="deliver_cores", name="Beast Core Requisition", kind="deliver", item="low_beast_core", target=5,
         reward=80, stones=8, desc="Deliver 5 Low-Grade Beast Cores."),
    dict(id="seclusion", name="Closed-Door Seclusion", kind="meditate", target=300, reward=70, stones=5,
         desc="Meditate for 300 seconds."),
    dict(id="refine_pills", name="Alchemy Practice", kind="alchemy", target=3, reward=70, stones=6,
         desc="Successfully refine 3 batches of pills."),
]
# treasury: id -> contribution cost, min rank index
SECT_TREASURY = [
    dict(item="qi_gathering_pill", count=3, cost=40, rank=0),
    dict(item="healing_pill", count=3, cost=30, rank=0),
    dict(item="scroll_fireball", count=1, cost=120, rank=0),
    dict(item="scroll_sword_qi", count=1, cost=120, rank=0),
    dict(item="scroll_wind_step", count=1, cost=100, rank=0),
    dict(item="manual_five_elements", count=1, cost=200, rank=0),
    dict(item="spirit_condensing_pill", count=2, cost=120, rank=1),
    dict(item="scroll_frost_spikes", count=1, cost=200, rank=1),
    dict(item="scroll_healing_spring", count=1, cost=200, rank=1),
    dict(item="scroll_thunder_palm", count=1, cost=300, rank=1),
    dict(item="manual_azure_wood", count=1, cost=500, rank=1),
    dict(item="manual_blazing_sun", count=1, cost=500, rank=1),
    dict(item="manual_profound_water", count=1, cost=500, rank=1),
    dict(item="manual_golden_vajra", count=1, cost=500, rank=1),
    dict(item="manual_great_earth", count=1, cost=500, rank=1),
    dict(item="foundation_pill", count=1, cost=400, rank=1),
    dict(item="scroll_golden_bell", count=1, cost=500, rank=2),
    dict(item="scroll_phoenix_flame", count=1, cost=700, rank=2),
    dict(item="scroll_buddha_palm", count=1, cost=800, rank=2),
    dict(item="golden_core_pill", count=1, cost=900, rank=2),
    dict(item="bone_marrow_pill", count=1, cost=1200, rank=2),
    dict(item="manual_nine_heavens_thunder", count=1, cost=2000, rank=3),
    dict(item="manual_frost_moon", count=1, cost=2000, rank=3),
    dict(item="manual_void_wind", count=1, cost=2000, rank=3),
    dict(item="scroll_void_step", count=1, cost=1500, rank=3),
    dict(item="scroll_sword_rain", count=1, cost=2500, rank=3),
    dict(item="nascent_soul_pill", count=1, cost=2500, rank=3),
    dict(item="scroll_heavenly_domain", count=1, cost=6000, rank=4),
    dict(item="void_tribulation_pill", count=1, cost=8000, rank=4),
    dict(item="manual_primordial_chaos", count=1, cost=30000, rank=5),
    dict(item="scroll_starfall", count=1, cost=25000, rank=5),
]

# ---------------------------------------------------------------------------
# Wandering merchant stock: (item, count, price in low spirit stones)
# ---------------------------------------------------------------------------
MERCHANT_STOCK = [
    ("qi_gathering_pill", 1, 12), ("healing_pill", 1, 8), ("bigu_pill", 2, 4), ("qi_recovery_pill", 1, 10),
    ("root_testing_stone", 1, 3), ("talisman_paper", 8, 6), ("cinnabar_ink", 2, 6),
    ("fire_talisman", 1, 10), ("protection_talisman", 1, 14), ("escape_talisman", 1, 20),
    ("scroll_qi_bolt", 1, 25), ("scroll_spirit_sense", 1, 30), ("scroll_vine_bind", 1, 60),
    ("scroll_tidal_palm", 1, 60), ("scroll_earth_shield", 1, 80), ("scroll_blood_sacrifice", 1, 120),
    ("manual_basic_breathing", 1, 15), ("manual_five_elements", 1, 90), ("manual_heavenly_devouring", 1, 400),
    ("spirit_fruit", 2, 8), ("immortal_peach", 1, 300), ("spirit_jade", 1, 20), ("profound_iron_ingot", 1, 15),
    ("dragon_summoning_pearl", 1, 900), ("beast_tide_horn", 1, 120), ("sect_token", 1, 10),
]
# Sell prices (low spirit stones per item) for the merchant buy-back
SELL_PRICES = {
    "low_beast_core": 3, "mid_beast_core": 12, "high_beast_core": 45, "king_beast_core": 200, "demon_core": 25,
    "dragon_scale": 60, "tribulation_essence": 80, "fox_spirit_tail": 15, "tiger_bone": 10, "wolf_fang": 2,
    "spirit_grass": 1, "blood_ginseng": 3, "frost_lotus": 4, "flame_lotus": 4, "purple_cloud_mushroom": 4,
    "moon_orchid": 4, "thunder_fern": 5, "jade_dew_flower": 3, "spirit_jade": 8, "raw_profound_iron": 3,
}

# ---------------------------------------------------------------------------
# Entities
# ---------------------------------------------------------------------------
ENTITIES = [
    dict(id="spirit_fox", name="Spirit Fox", model="fox", health=24, damage=4, speed=0.32, family=["spirit_beast",
         "fox_spirit", "mob"], hostile=False, tameable=True, egg=((240, 240, 255), (110, 200, 255)),
         spawn=dict(biomes=["taiga", "forest", "cherry_grove", "frozen"], weight=6, herd=(1, 2), surface=True)),
    dict(id="demonic_wolf", name="Demonic Wolf", model="wolf", health=30, damage=6, speed=0.34,
         family=["spirit_beast", "demonic_beast", "monster", "mob"], hostile=True,
         egg=((40, 40, 50), (220, 30, 40)),
         spawn=dict(biomes=["forest", "taiga", "roofed", "plains", "extreme_hills"], weight=18, herd=(2, 4),
                    surface=True, night=True)),
    dict(id="flame_tiger", name="Flame-Striped Tiger", model="tiger", health=60, damage=9, speed=0.3,
         family=["spirit_beast", "demonic_beast", "monster", "mob"], hostile=True,
         egg=((240, 130, 40), (30, 20, 20)),
         spawn=dict(biomes=["savanna", "mesa", "jungle", "desert"], weight=6, herd=(1, 1), surface=True)),
    dict(id="flood_dragon", name="Azure Flood Dragon", model="dragon", health=600, damage=16, speed=0.28,
         family=["spirit_beast", "boss", "monster", "mob"], hostile=True, boss=True,
         egg=((40, 150, 190), (240, 220, 90)), spawn=None),
    dict(id="rogue_cultivator", name="Rogue Cultivator", model="humanoid", health=40, damage=6, speed=0.3,
         family=["cultivator", "rogue_cultivator", "monster", "mob"], hostile=True,
         egg=((90, 90, 110), (110, 200, 255)),
         spawn=dict(biomes=["plains", "forest", "taiga", "savanna", "extreme_hills", "meadow"], weight=5,
                    herd=(1, 1), surface=True)),
    dict(id="demonic_cultivator", name="Demonic Cultivator", model="humanoid", health=60, damage=8, speed=0.3,
         family=["cultivator", "demonic_cultivator", "monster", "mob"], hostile=True,
         egg=((40, 10, 20), (200, 20, 40)),
         spawn=dict(biomes=["roofed", "swamp", "taiga", "desert", "mesa", "plains"], weight=4, herd=(1, 1),
                    surface=True, night=True)),
    dict(id="wandering_merchant", name="Wandering Pill Merchant", model="humanoid", health=40, damage=0, speed=0.25,
         family=["cultivator", "merchant", "mob"], hostile=False, egg=((200, 170, 90), (90, 50, 30)),
         spawn=dict(biomes=["plains", "forest", "savanna", "desert", "meadow", "taiga"], weight=2, herd=(1, 1),
                    surface=True, day=True)),
    dict(id="heart_demon", name="Heart Demon", model="humanoid", health=80, damage=10, speed=0.34,
         family=["heart_demon", "monster", "mob"], hostile=True, egg=((10, 10, 15), (140, 40, 200)), spawn=None),
]
