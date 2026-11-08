import { expect, test } from "bun:test";
import { songTitle } from "./titles.ts";

test("GUS 8.3 names map to title and credit", () => {
  expect(songTitle("gus-hotelcal", "hotelcal")).toEqual({ title: "Hotel California", by: "Eagles" });
  expect(songTitle("gus-unknown", "some song").title).toBe("Some Song");
});

test("Novak slugs become Game — Track", () => {
  expect(songTitle("novak-doom-e1m1", "").title).toBe("Doom — E1M1 · At Doom's Gate");
  expect(songTitle("novak-quest-for-glory-iii-apothecary-s-hut", "").title).toBe("Quest for Glory III — Apothecary's Hut");
  expect(songTitle("novak-descent-l01-lunar-outpost", "").title).toBe("Descent — Level 01 · Lunar Outpost");
  expect(songTitle("novak-warcraft2-orc1", "").title).toBe("Warcraft II — Orc 1");
  expect(songTitle("novak-sam-n-max", "").title).toBe("Sam & Max Hit the Road");
});
