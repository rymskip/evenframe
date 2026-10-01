// Decodes JSON the way serde writes it, through the ArkType and Effect output
// generated for the `vec_and_map` and `durations` fixtures, and fails on any
// disagreement.
import { validator } from "./arktype_vec_and_map.ts";
import { Collection } from "./effect_vec_and_map.ts";
import { validator as durationValidator } from "./arktype_durations.ts";
import { Timer } from "./effect_durations.ts";
import { Schema } from "effect";
import { type } from "arktype";

const base = {
  id: "c1",
  tags: [],
  scores: [],
  metadata: { "": "blank key is a valid String key" },
  sortedData: {},
  notesByRank: { "3": "third", "10": null },
  relatedByName: { twin: { table: "collection", key: { String: "2" } } },
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
  { name: "record link with a number key", value: { ...base, relatedByName: { twin: { table: "collection", key: { Number: 2 } } } }, accepted: true },
  { name: "record link with a uuid key", value: { ...base, relatedByName: { twin: { table: "collection", key: { Uuid: "0190d9df-a1b2-7c3d-8e4f-5a6b7c8d9e0f" } } } }, accepted: true },
  { name: "record link to the record itself", value: { ...base, relatedByName: { twin: base } }, accepted: true },
  { name: "record link key of no SDK kind", value: { ...base, relatedByName: { twin: { table: "collection", key: { Float: 2.5 } } } }, accepted: false },
  { name: "record link without a table", value: { ...base, relatedByName: { twin: { key: { String: "2" } } } }, accepted: false },
];

const timer = { id: "t1", limit: { secs: 3600, nanos: 0 }, grace: null, laps: [] };

const timerCases: Case[] = [
  { name: "every duration valid", value: timer, accepted: true },
  { name: "durations in a list", value: { ...timer, laps: [{ secs: 0, nanos: 999999999 }] }, accepted: true },
  { name: "duration over its bound", value: { ...timer, limit: { secs: 8 * 3600, nanos: 1 } }, accepted: false },
  { name: "duration without nanos", value: { ...timer, limit: { secs: 60 } }, accepted: false },
  { name: "duration with another key", value: { ...timer, limit: { secs: 60, nanos: 0, millis: 0 } }, accepted: false },
  { name: "duration nanos of a whole second", value: { ...timer, limit: { secs: 60, nanos: 1000000000 } }, accepted: false },
  { name: "negative duration", value: { ...timer, limit: { secs: -1, nanos: 0 } }, accepted: false },
  { name: "duration as text", value: { ...timer, limit: "1h" }, accepted: false },
  { name: "optional duration under its bound", value: { ...timer, grace: { secs: 30, nanos: 0 } }, accepted: false },
  { name: "optional duration over its bound", value: { ...timer, grace: { secs: 120, nanos: 0 } }, accepted: true },
];

let failures = 0;
const check = <A, I>(
  cases: Case[],
  ark: (value: unknown) => unknown,
  effect: Schema.Schema<A, I, never>,
) => {
  for (const testCase of cases) {
    const arkResult = ark(testCase.value);
    const effectResult = Schema.decodeUnknownEither(effect)(testCase.value);
    const outcomes: { library: string; accepted: boolean; detail: string }[] = [
      {
        library: "arktype",
        accepted: !(arkResult instanceof type.errors),
        detail: arkResult instanceof type.errors ? arkResult.summary : "",
      },
      {
        library: "effect",
        accepted: effectResult._tag === "Right",
        detail: effectResult._tag === "Left" ? String(effectResult.left) : "",
      },
    ];
    for (const { library, accepted, detail } of outcomes) {
      const ok = accepted === testCase.accepted;
      if (!ok) failures += 1;
      console.log(`${ok ? "ok  " : "FAIL"} ${library} ${testCase.name}: ${accepted ? "accepted" : "rejected"}${ok ? "" : ` (expected ${testCase.accepted ? "accepted" : "rejected"}) ${detail}`}`);
    }
  }
};
check(cases, (value) => validator.Collection(value), Collection);
check(timerCases, (value) => durationValidator.Timer(value), Timer);
console.log(failures === 0 ? "all cases behave as serde does" : `${failures} case(s) disagree with serde`);
if (failures > 0) Deno.exit(1);
