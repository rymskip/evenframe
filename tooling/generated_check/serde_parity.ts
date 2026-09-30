// Decodes JSON the way serde writes it, through the ArkType and Effect output
// generated for the `vec_and_map` fixture, and fails on any disagreement.
import { validator } from "./arktype_vec_and_map.ts";
import { Collection } from "./effect_vec_and_map.ts";
import { Schema } from "effect";
import { type } from "arktype";

const base = {
  id: "c1",
  tags: [],
  scores: [],
  metadata: { "": "blank key is a valid String key" },
  sortedData: {},
  notesByRank: { "3": "third", "10": null },
  relatedByName: { twin: "collection:2" },
  balanceByOffset: { "-5": 1.5, "0": 2 },
  countByLetter: { a: 1, "😀": 2, "\n": 3 },
  ownerByRole: { Admin: "ada" },
  labelByAccount: { "acct-9": "main" },
  grid: { "1": { "2": true } },
  noteByFlag: { "true": "on" },
  initial: "😀",
};

type Case = { name: string; value: Record<string, unknown>; accepted: boolean };

const cases: Case[] = [
  { name: "every field valid", value: base, accepted: true },
  { name: "integer key with a leading zero", value: { ...base, balanceByOffset: { "05": 1 } }, accepted: false },
  { name: "integer key with a fraction", value: { ...base, balanceByOffset: { "1.5": 1 } }, accepted: false },
  { name: "integer key with a plus sign", value: { ...base, balanceByOffset: { "+5": 1 } }, accepted: false },
  { name: "word as an integer key", value: { ...base, notesByRank: { three: "x" } }, accepted: false },
  { name: "two characters as a char key", value: { ...base, countByLetter: { ab: 1 } }, accepted: false },
  { name: "empty char key", value: { ...base, countByLetter: { "": 1 } }, accepted: false },
  { name: "enum key outside the enum", value: { ...base, ownerByRole: { Nobody: "x" } }, accepted: false },
  { name: "bad key in a nested map", value: { ...base, grid: { "1": { x: true } } }, accepted: false },
  { name: "bad outer key of a nested map", value: { ...base, grid: { x: { "1": true } } }, accepted: false },
  { name: "both bool keys", value: { ...base, noteByFlag: { "true": "on", "false": "off" } }, accepted: true },
  { name: "no bool keys", value: { ...base, noteByFlag: {} }, accepted: true },
  { name: "capitalized bool key", value: { ...base, noteByFlag: { True: "on" } }, accepted: false },
  { name: "digit as a bool key", value: { ...base, noteByFlag: { "1": "on" } }, accepted: false },
  { name: "char field of one ASCII character", value: { ...base, initial: "a" }, accepted: true },
  { name: "char field of a newline", value: { ...base, initial: "\n" }, accepted: true },
  { name: "empty char field", value: { ...base, initial: "" }, accepted: false },
  { name: "two-character char field", value: { ...base, initial: "ab" }, accepted: false },
  { name: "two astral characters as a char field", value: { ...base, initial: "😀😀" }, accepted: false },
];

let failures = 0;
for (const testCase of cases) {
  const ark = validator.Collection(testCase.value);
  const arkAccepted = !(ark instanceof type.errors);
  const effect = Schema.decodeUnknownEither(Collection)(testCase.value);
  const effectAccepted = effect._tag === "Right";
  const outcomes: { library: string; accepted: boolean; detail: string }[] = [
    { library: "arktype", accepted: arkAccepted, detail: ark instanceof type.errors ? ark.summary : "" },
    { library: "effect", accepted: effectAccepted, detail: effect._tag === "Left" ? String(effect.left) : "" },
  ];
  for (const { library, accepted, detail } of outcomes) {
    const ok = accepted === testCase.accepted;
    if (!ok) failures += 1;
    console.log(`${ok ? "ok  " : "FAIL"} ${library} ${testCase.name}: ${accepted ? "accepted" : "rejected"}${ok ? "" : ` (expected ${testCase.accepted ? "accepted" : "rejected"}) ${detail}`}`);
  }
}
console.log(failures === 0 ? "all cases behave as serde does" : `${failures} case(s) disagree with serde`);
if (failures > 0) Deno.exit(1);
