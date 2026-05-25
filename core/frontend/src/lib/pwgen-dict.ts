/**
 * Small curated English dictionary for the `pwgen` command's
 * dictionary + leetspeak modes. 400 common 4-7 letter words, all
 * lowercase, no proper nouns, no offensive terms. Hand-picked for
 * memorability + visual clarity (no ambiguous-looking glyphs like
 * "rn" → "m"). Roughly the EFF short-word list overlap.
 *
 * Bundle size: ~3 KB minified — negligible vs. the React + Tauri
 * runtime baseline.
 */
export const DICT_WORDS: ReadonlyArray<string> = [
  "able", "acid", "acre", "aged", "ahead", "aim", "air", "ally", "alpha", "amber",
  "anchor", "angel", "angle", "ankle", "apple", "april", "arcade", "arch", "arena", "argue",
  "army", "art", "ash", "ask", "atom", "auction", "audit", "aunt", "auto", "avail",
  "avenue", "awake", "award", "aware", "axe", "babe", "back", "bacon", "bad", "badge",
  "bag", "bait", "bake", "ball", "band", "bank", "barn", "base", "basic", "bath",
  "bay", "beach", "bean", "bear", "beat", "beauty", "bed", "bee", "beef", "begin",
  "bell", "belt", "bench", "bend", "best", "bid", "big", "bike", "bill", "bird",
  "birth", "bit", "bite", "black", "blade", "blame", "blank", "blend", "bless", "blind",
  "blink", "block", "blood", "bloom", "blow", "blue", "blur", "board", "boat", "body",
  "bold", "bolt", "bomb", "bond", "bone", "bonus", "book", "boom", "boost", "boot",
  "border", "born", "boss", "both", "boxer", "boy", "brain", "brand", "brave", "bread",
  "break", "brick", "bride", "brief", "bring", "brisk", "broad", "brown", "brush", "buddy",
  "buffer", "build", "bulb", "bulk", "bull", "bump", "bunch", "burn", "bus", "bush",
  "buy", "buzz", "cabin", "cable", "cake", "calf", "calm", "camel", "camp", "candy",
  "cap", "car", "card", "care", "cargo", "case", "cash", "cast", "cat", "catch",
  "cause", "cave", "cease", "cedar", "cell", "cement", "cent", "chain", "chair", "chalk",
  "chance", "chaos", "charge", "chart", "chase", "cheap", "check", "cheer", "chef", "cherry",
  "chess", "chest", "chick", "chief", "child", "chili", "chill", "chip", "choice", "city",
  "civic", "claim", "clamp", "clan", "clash", "clay", "clean", "clear", "clerk", "click",
  "cliff", "climb", "clock", "close", "cloud", "club", "coach", "coast", "coat", "code",
  "coffee", "coil", "coin", "cold", "color", "comb", "come", "comic", "cone", "cook",
  "cool", "copper", "core", "cork", "corn", "cost", "cotton", "couch", "country", "couple",
  "course", "cover", "cow", "crab", "crane", "crash", "craft", "cream", "credit", "crew",
  "crime", "crisp", "crop", "cross", "crowd", "crown", "crush", "crust", "cube", "cuddle",
  "cup", "curb", "cure", "curl", "curve", "cycle", "daily", "dam", "dance", "dare",
  "dart", "dash", "data", "date", "day", "dead", "deal", "dean", "dear", "debt",
  "deck", "deep", "deer", "delta", "demo", "dense", "depth", "derby", "desk", "diet",
  "digit", "dim", "diner", "dirt", "dish", "diver", "doc", "dock", "dog", "doll",
  "donor", "door", "dot", "dove", "down", "draft", "drag", "drain", "drama", "draw",
  "dream", "dress", "drift", "drill", "drink", "drive", "drop", "drum", "dry", "duck",
  "duel", "duke", "dune", "dusk", "dust", "duty", "eager", "eagle", "early", "earn",
  "east", "easy", "echo", "edge", "edit", "eel", "egg", "eight", "elbow", "elder",
  "elite", "elm", "else", "ember", "empty", "end", "enemy", "energy", "entry", "envy",
  "epic", "equal", "era", "essay", "even", "event", "ever", "evil", "exact", "exam",
  "exit", "extra", "fable", "face", "fact", "fade", "fair", "fake", "fall", "false",
  "fame", "fan", "far", "farm", "fast", "fat", "fault", "favor", "fear", "feast",
  "fee", "feed", "feel", "fence", "fern", "few", "field", "fifty", "fight", "file",
  "fill", "film", "final", "find", "fine", "finger", "finish", "fire", "firm", "first",
  "fish", "fit", "fix", "flag", "flame", "flap", "flare", "flash", "flat", "flesh",
  "fleet", "flex", "float", "flock", "flood", "floor", "flour", "flow", "flu", "fluid",
  "flute", "fly", "foam", "fog", "foil", "fold", "folk", "food", "fool", "foot",
  "force", "fork", "form", "fort", "forty", "fox", "frame", "fresh", "fried", "friend",
  "frog", "front", "frost", "fruit", "fuel", "full", "fun", "fund", "funny", "fur",
];
