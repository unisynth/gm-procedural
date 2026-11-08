// Display titles for the demo songs. The render ids are file names (Novak: game-track slugs; GUS: 8.3 DOS names),
// so the readable title and credit come from here. GUS credits come from the files' own meta text where present.
const GUS: Record<string, [string, string]> = {
  "06twelv-": ["Twelve Days of Christmas", "traditional"],
  africa: ["Africa", "Toto"],
  boesendo: ["WinJammer Demo", "WinJammer"],
  calyp32: ["Calypso 1", "Jean-Michel Jarre (LAPC-1/CM-32L arrangement)"],
  calypso2: ["Calypso 2", "Jean-Michel Jarre"],
  close2me: ["Close to Me", "The Cure"],
  cyber: ["Cyber", "GUS collection"],
  eb4: ["EB4", "GUS collection"],
  echoes1: ["Echoes", "GUS collection"],
  equinox4: ["Équinoxe 4", "Jean-Michel Jarre"],
  etude: ["Etude", "GUS collection"],
  eve: ["Eve", "GUS collection"],
  "h-rag": ["H-Rag", "GUS collection"],
  hotelcal: ["Hotel California", "Eagles"],
  "l1002-02": ["Wapiko Genkiyoho", "© 1991 Roland Corporation"],
  "l1002-08": ["Odoru Ponpokorin", "© 1991 Roland Corporation"],
  midi1: ["PC Player Testsong", "DoReMiX"],
  orientex: ["Orient Express", "GUS collection"],
  "psr510-4": ["PSR-510 Demo 4", "Yamaha"],
  shaft: ["Theme from Shaft", "Isaac Hayes"],
  undersea: ["Under the Sea", "Alan Menken"],
  videokil: ["Video Killed the Radio Star", "The Buggles"],
  whatislo: ["What Is Love", "Haddaway"],
  wind: ["The Wind Beneath My Wings", "sequenced by Walter June"],
};

// Novak slugs start with the game; the rest is the track.
const GAMES: [string, string][] = [
  ["arena", "The Elder Scrolls: Arena"], ["azraels-tear", "Azrael's Tear"], ["betrayal-at-krondor", "Betrayal at Krondor"],
  ["death-gate", "Death Gate"], ["descent", "Descent"], ["discworld", "Discworld"], ["doom-e1m1", "Doom — E1M1 · At Doom's Gate"], ["doom", "Doom"],
  ["duke-nukem-3d", "Duke Nukem 3D"], ["gabriel-knight", "Gabriel Knight"], ["leisure-suit-larry-6", "Leisure Suit Larry 6"],
  ["quest-for-glory-iii", "Quest for Glory III"], ["realms-of-arkania-2", "Realms of Arkania 2"], ["sam-n-max", "Sam & Max Hit the Road"],
  ["shadow-warrior", "Shadow Warrior"], ["space-quest-v", "Space Quest V"], ["stonekeep", "Stonekeep"], ["system-shock", "System Shock"],
  ["tie-fighter", "TIE Fighter"], ["transport-tycoon-deluxe", "Transport Tycoon Deluxe"], ["under-a-killing-moon", "Under a Killing Moon"],
  ["warcraft2", "Warcraft II"],
];

const cap = (s: string) => s.replace(/\b([a-z])/g, (m) => m.toUpperCase()).replace(/\bS\b/g, "s").replace(/\b(Iii|Ii)\b/g, (m) => m.toUpperCase());

export function songTitle(id: string, fallback: string): { title: string; by: string } {
  const [src, ...rest] = id.split("-");
  const slug = rest.join("-");
  if (src === "gus") {
    const g = GUS[slug];
    return g ? { title: g[0], by: g[1] } : { title: cap(fallback), by: "GUS collection" };
  }
  for (const [k, game] of GAMES) {
    if (slug === k) return { title: game, by: "DOS game soundtrack" };
    if (slug.startsWith(k + "-")) {
      const track = slug.slice(k.length + 1).replace(/^l(\d+)-/, "Level $1 · ").replace(/-(\d)$/, " $1").replace(/([a-z])(\d)$/, "$1 $2")
        .replace(/-s-/g, "'s-").replace(/unversity/, "university").replace(/-/g, " ");
      return { title: `${game} — ${cap(track).replace(/'S\b/g, "'s").replace(/ (Of|The|A) /g, (m) => m.toLowerCase()).replace(/^E1m1$|E1m1/, "E1M1")}`, by: "DOS game soundtrack" };
    }
  }
  return { title: cap(slug.replace(/-/g, " ")), by: "DOS game soundtrack" };
}
