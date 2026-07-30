/**
 * A curated, bundled list of major world cities for the `weather` command's
 * city autocomplete (v0.100.0). Static + offline — no per-keystroke geocoding
 * call. The completion fills just the city name (`weather Darmstadt`); the
 * backend queries OpenWeather with `q=<name>`, which resolves it. `pop` is an
 * approximate population (thousands) used only to rank matches — bigger cities
 * first — so `weather d` surfaces Delhi/Dubai/Dortmund before tiny towns.
 *
 * German cities are covered down to ~100k (the maintainer's locale) alongside
 * world capitals + metros; it is intentionally NOT exhaustive — a city missing
 * from the list still works, it just doesn't autocomplete (the raw command runs
 * and OpenWeather tries the typed name).
 */
export interface City {
  name: string;
  /** Short country/region label shown for context (not part of the query). */
  country: string;
  /** Approximate population in thousands — for ranking matches only. */
  pop: number;
}

// Kept as tuples for a compact source; expanded to `City` below.
const RAW: ReadonlyArray<readonly [string, string, number]> = [
  // ── Germany (down to ~100k) ──────────────────────────────────────────────
  ["Berlin", "Germany", 3677], ["Hamburg", "Germany", 1906], ["München", "Germany", 1512],
  ["Köln", "Germany", 1073], ["Frankfurt", "Germany", 773], ["Stuttgart", "Germany", 630],
  ["Düsseldorf", "Germany", 620], ["Leipzig", "Germany", 597], ["Dortmund", "Germany", 588],
  ["Essen", "Germany", 583], ["Bremen", "Germany", 566], ["Dresden", "Germany", 556],
  ["Hannover", "Germany", 535], ["Nürnberg", "Germany", 518], ["Duisburg", "Germany", 495],
  ["Bochum", "Germany", 365], ["Wuppertal", "Germany", 355], ["Bielefeld", "Germany", 334],
  ["Bonn", "Germany", 331], ["Münster", "Germany", 316], ["Mannheim", "Germany", 311],
  ["Karlsruhe", "Germany", 306], ["Augsburg", "Germany", 296], ["Wiesbaden", "Germany", 279],
  ["Mönchengladbach", "Germany", 261], ["Gelsenkirchen", "Germany", 260], ["Aachen", "Germany", 249],
  ["Braunschweig", "Germany", 249], ["Chemnitz", "Germany", 246], ["Kiel", "Germany", 246],
  ["Halle", "Germany", 238], ["Magdeburg", "Germany", 237], ["Freiburg", "Germany", 231],
  ["Krefeld", "Germany", 227], ["Mainz", "Germany", 218], ["Lübeck", "Germany", 216],
  ["Erfurt", "Germany", 214], ["Oberhausen", "Germany", 210], ["Rostock", "Germany", 209],
  ["Kassel", "Germany", 202], ["Hagen", "Germany", 189], ["Potsdam", "Germany", 183],
  ["Saarbrücken", "Germany", 180], ["Hamm", "Germany", 179], ["Ludwigshafen", "Germany", 172],
  ["Mülheim", "Germany", 171], ["Oldenburg", "Germany", 170], ["Osnabrück", "Germany", 165],
  ["Leverkusen", "Germany", 164], ["Darmstadt", "Germany", 162], ["Solingen", "Germany", 160],
  ["Heidelberg", "Germany", 160], ["Herne", "Germany", 156], ["Neuss", "Germany", 153],
  ["Regensburg", "Germany", 153], ["Paderborn", "Germany", 151], ["Ingolstadt", "Germany", 137],
  ["Würzburg", "Germany", 127], ["Fürth", "Germany", 128], ["Wolfsburg", "Germany", 124],
  ["Offenbach", "Germany", 130], ["Ulm", "Germany", 126], ["Heilbronn", "Germany", 126],
  ["Pforzheim", "Germany", 125], ["Göttingen", "Germany", 118], ["Bottrop", "Germany", 117],
  ["Trier", "Germany", 111], ["Recklinghausen", "Germany", 110], ["Reutlingen", "Germany", 116],
  ["Bremerhaven", "Germany", 114], ["Koblenz", "Germany", 114], ["Bergisch Gladbach", "Germany", 111],
  ["Jena", "Germany", 111], ["Remscheid", "Germany", 111], ["Erlangen", "Germany", 112],
  ["Moers", "Germany", 104], ["Siegen", "Germany", 102], ["Hildesheim", "Germany", 101],
  ["Salzgitter", "Germany", 104], ["Kaiserslautern", "Germany", 100], ["Cottbus", "Germany", 99],
  // ── Austria / Switzerland ────────────────────────────────────────────────
  ["Wien", "Austria", 1920], ["Graz", "Austria", 291], ["Linz", "Austria", 206],
  ["Salzburg", "Austria", 156], ["Innsbruck", "Austria", 132], ["Klagenfurt", "Austria", 101],
  ["Zürich", "Switzerland", 421], ["Genève", "Switzerland", 203], ["Basel", "Switzerland", 178],
  ["Bern", "Switzerland", 134], ["Lausanne", "Switzerland", 140],
  // ── Europe ───────────────────────────────────────────────────────────────
  ["London", "United Kingdom", 8982], ["Birmingham", "United Kingdom", 1141],
  ["Manchester", "United Kingdom", 553], ["Glasgow", "United Kingdom", 635],
  ["Liverpool", "United Kingdom", 496], ["Edinburgh", "United Kingdom", 524],
  ["Leeds", "United Kingdom", 789], ["Bristol", "United Kingdom", 467],
  ["Paris", "France", 2161], ["Marseille", "France", 861], ["Lyon", "France", 516],
  ["Toulouse", "France", 479], ["Nice", "France", 342], ["Nantes", "France", 309],
  ["Strasbourg", "France", 280], ["Bordeaux", "France", 254], ["Lille", "France", 233],
  ["Madrid", "Spain", 3223], ["Barcelona", "Spain", 1620], ["Valencia", "Spain", 791],
  ["Sevilla", "Spain", 688], ["Zaragoza", "Spain", 675], ["Málaga", "Spain", 574],
  ["Bilbao", "Spain", 345], ["Palma", "Spain", 409],
  ["Roma", "Italy", 2873], ["Milano", "Italy", 1352], ["Napoli", "Italy", 962],
  ["Torino", "Italy", 870], ["Palermo", "Italy", 663], ["Genova", "Italy", 580],
  ["Bologna", "Italy", 391], ["Firenze", "Italy", 383], ["Venezia", "Italy", 258],
  ["Lisboa", "Portugal", 505], ["Porto", "Portugal", 214],
  ["Amsterdam", "Netherlands", 872], ["Rotterdam", "Netherlands", 651],
  ["Den Haag", "Netherlands", 545], ["Utrecht", "Netherlands", 358],
  ["Brussels", "Belgium", 1209], ["Antwerp", "Belgium", 529], ["Ghent", "Belgium", 263],
  ["Dublin", "Ireland", 555], ["Copenhagen", "Denmark", 794], ["Stockholm", "Sweden", 975],
  ["Gothenburg", "Sweden", 580], ["Oslo", "Norway", 697], ["Helsinki", "Finland", 656],
  ["Reykjavik", "Iceland", 131], ["Warsaw", "Poland", 1790], ["Kraków", "Poland", 780],
  ["Łódź", "Poland", 672], ["Wrocław", "Poland", 641], ["Poznań", "Poland", 534],
  ["Gdańsk", "Poland", 470], ["Prague", "Czechia", 1309], ["Brno", "Czechia", 382],
  ["Budapest", "Hungary", 1752], ["Bratislava", "Slovakia", 438], ["Vienna", "Austria", 1920],
  ["Bucharest", "Romania", 1716], ["Sofia", "Bulgaria", 1242], ["Belgrade", "Serbia", 1166],
  ["Zagreb", "Croatia", 767], ["Ljubljana", "Slovenia", 285], ["Athens", "Greece", 664],
  ["Thessaloniki", "Greece", 315], ["Lisbon", "Portugal", 505], ["Kyiv", "Ukraine", 2963],
  ["Kharkiv", "Ukraine", 1443], ["Moscow", "Russia", 12655], ["Saint Petersburg", "Russia", 5384],
  ["Istanbul", "Turkey", 15462], ["Ankara", "Turkey", 5663], ["Izmir", "Turkey", 4394],
  // ── North America ────────────────────────────────────────────────────────
  ["New York", "United States", 8336], ["Los Angeles", "United States", 3979],
  ["Chicago", "United States", 2693], ["Houston", "United States", 2320],
  ["Phoenix", "United States", 1680], ["Philadelphia", "United States", 1584],
  ["San Antonio", "United States", 1547], ["San Diego", "United States", 1424],
  ["Dallas", "United States", 1343], ["San Jose", "United States", 1021],
  ["Austin", "United States", 978], ["San Francisco", "United States", 874],
  ["Seattle", "United States", 753], ["Denver", "United States", 727],
  ["Boston", "United States", 692], ["Washington", "United States", 705],
  ["Las Vegas", "United States", 651], ["Portland", "United States", 654],
  ["Miami", "United States", 467], ["Atlanta", "United States", 498],
  ["Toronto", "Canada", 2930], ["Montreal", "Canada", 1780], ["Vancouver", "Canada", 675],
  ["Calgary", "Canada", 1306], ["Ottawa", "Canada", 1017], ["Edmonton", "Canada", 981],
  ["Mexico City", "Mexico", 9209], ["Guadalajara", "Mexico", 1495],
  ["Monterrey", "Mexico", 1142], ["Tijuana", "Mexico", 1810], ["Puebla", "Mexico", 1692],
  // ── South America ────────────────────────────────────────────────────────
  ["São Paulo", "Brazil", 12325], ["Rio de Janeiro", "Brazil", 6748],
  ["Brasília", "Brazil", 3055], ["Salvador", "Brazil", 2886], ["Fortaleza", "Brazil", 2687],
  ["Buenos Aires", "Argentina", 3075], ["Córdoba", "Argentina", 1391],
  ["Lima", "Peru", 9752], ["Bogotá", "Colombia", 7413], ["Medellín", "Colombia", 2529],
  ["Santiago", "Chile", 6158], ["Caracas", "Venezuela", 2245], ["Quito", "Ecuador", 2011],
  ["Montevideo", "Uruguay", 1319],
  // ── Africa ───────────────────────────────────────────────────────────────
  ["Cairo", "Egypt", 9540], ["Alexandria", "Egypt", 5200], ["Lagos", "Nigeria", 9000],
  ["Kinshasa", "DR Congo", 14970], ["Johannesburg", "South Africa", 5635],
  ["Cape Town", "South Africa", 4618], ["Durban", "South Africa", 3442],
  ["Nairobi", "Kenya", 4397], ["Casablanca", "Morocco", 3359], ["Marrakesh", "Morocco", 928],
  ["Addis Ababa", "Ethiopia", 3384], ["Accra", "Ghana", 2388], ["Dakar", "Senegal", 1146],
  ["Tunis", "Tunisia", 638], ["Algiers", "Algeria", 2364],
  // ── Middle East ──────────────────────────────────────────────────────────
  ["Dubai", "UAE", 3331], ["Abu Dhabi", "UAE", 1483], ["Riyadh", "Saudi Arabia", 7676],
  ["Jeddah", "Saudi Arabia", 4697], ["Doha", "Qatar", 2382], ["Kuwait City", "Kuwait", 3115],
  ["Tel Aviv", "Israel", 460], ["Jerusalem", "Israel", 936], ["Amman", "Jordan", 4007],
  ["Beirut", "Lebanon", 2424], ["Baghdad", "Iraq", 7145], ["Tehran", "Iran", 8694],
  // ── Asia ─────────────────────────────────────────────────────────────────
  ["Tokyo", "Japan", 13960], ["Yokohama", "Japan", 3726], ["Osaka", "Japan", 2691],
  ["Nagoya", "Japan", 2296], ["Sapporo", "Japan", 1952], ["Kyoto", "Japan", 1475],
  ["Seoul", "South Korea", 9776], ["Busan", "South Korea", 3449], ["Incheon", "South Korea", 2954],
  ["Beijing", "China", 21540], ["Shanghai", "China", 24280], ["Guangzhou", "China", 14904],
  ["Shenzhen", "China", 12530], ["Chengdu", "China", 16330], ["Wuhan", "China", 11080],
  ["Hong Kong", "China", 7482], ["Xi'an", "China", 12950], ["Hangzhou", "China", 10360],
  ["Delhi", "India", 16787], ["Mumbai", "India", 12442], ["Bangalore", "India", 8443],
  ["Hyderabad", "India", 6810], ["Chennai", "India", 7088], ["Kolkata", "India", 4497],
  ["Ahmedabad", "India", 5570], ["Pune", "India", 3124], ["Jaipur", "India", 3046],
  ["Bangkok", "Thailand", 10539], ["Singapore", "Singapore", 5686],
  ["Kuala Lumpur", "Malaysia", 1808], ["Jakarta", "Indonesia", 10562],
  ["Surabaya", "Indonesia", 2874], ["Bandung", "Indonesia", 2444],
  ["Manila", "Philippines", 1780], ["Quezon City", "Philippines", 2936],
  ["Ho Chi Minh City", "Vietnam", 8993], ["Hanoi", "Vietnam", 8054],
  ["Dhaka", "Bangladesh", 8906], ["Karachi", "Pakistan", 14910], ["Lahore", "Pakistan", 11130],
  ["Islamabad", "Pakistan", 1015], ["Colombo", "Sri Lanka", 752], ["Kathmandu", "Nepal", 975],
  ["Taipei", "Taiwan", 2646], ["Almaty", "Kazakhstan", 1977], ["Tashkent", "Uzbekistan", 2572],
  // ── Oceania ──────────────────────────────────────────────────────────────
  ["Sydney", "Australia", 5312], ["Melbourne", "Australia", 5078], ["Brisbane", "Australia", 2560],
  ["Perth", "Australia", 2085], ["Adelaide", "Australia", 1345], ["Canberra", "Australia", 431],
  ["Auckland", "New Zealand", 1657], ["Wellington", "New Zealand", 215],
];

export const MAJOR_CITIES: readonly City[] = RAW.map(([name, country, pop]) => ({
  name,
  country,
  pop,
}));

/** Lowercase + strip diacritics, so `munchen` matches `München`, `sao` `São`. */
export function normalizeCity(s: string): string {
  return s
    .trim()
    .toLowerCase()
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, ""); // strip combining diacritics
}

/**
 * Match `query` against the city list, best-first: **prefix** matches before
 * **substring** matches, each ranked by population (bigger city first). Returns
 * up to `limit`. Diacritic- and case-insensitive. Empty query → `[]`.
 */
export function matchCities(query: string, limit = 6): City[] {
  const q = normalizeCity(query);
  if (!q) return [];
  const prefix: City[] = [];
  const infix: City[] = [];
  for (const c of MAJOR_CITIES) {
    const n = normalizeCity(c.name);
    if (n.startsWith(q)) prefix.push(c);
    else if (n.includes(q)) infix.push(c);
  }
  const byPop = (a: City, b: City) => b.pop - a.pop;
  prefix.sort(byPop);
  infix.sort(byPop);
  return [...prefix, ...infix].slice(0, Math.max(0, limit));
}
