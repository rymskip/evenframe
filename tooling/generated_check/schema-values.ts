// Validates camelCase keys, explicit serde renames, serde-compatible value
// shapes and newtype validators through the generated ArkType and Effect
// schemas.
import { validator } from "./arktype_vec_and_map.ts";
import { Collection } from "./effect_vec_and_map.ts";
import { validator as durationValidator } from "./arktype_durations.ts";
import { Timer } from "./effect_durations.ts";
import { validator as memberValidator } from "./arktype_serde_names.ts";
import { Member } from "./effect_serde_names.ts";
import { validator as profileValidator } from "./arktype_newtypes.ts";
import { Profile } from "./effect_newtypes.ts";
import { validator as contactValidator } from "./arktype_enum_partially_untagged.ts";
import { Contact } from "./effect_enum_partially_untagged.ts";
import { validator as flattenValidator } from "./arktype_flatten.ts";
import { Event, Post, Settings } from "./effect_flatten.ts";
import { validator as tupleValidator } from "./arktype_tuple_elements.ts";
import { Range, Shape } from "./effect_tuple_elements.ts";
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

type Case = { name: string; value: unknown; accepted: boolean };

const cases: Array<Case> = [
  { name: "every field valid", value: base, accepted: true },
  {
    name: "Rust field spelling without the required TS key",
    value: { ...base, sortedData: undefined, sorted_data: {} },
    accepted: false,
  },
  {
    name: "integer key with a leading zero",
    value: { ...base, balanceByOffset: { "05": 1 } },
    accepted: false,
  },
  {
    name: "integer key with a fraction",
    value: { ...base, balanceByOffset: { "1.5": 1 } },
    accepted: false,
  },
  {
    name: "integer key with a plus sign",
    value: { ...base, balanceByOffset: { "+5": 1 } },
    accepted: false,
  },
  {
    name: "word as an integer key",
    value: { ...base, notesByRank: { three: "x" } },
    accepted: false,
  },
  {
    name: "two characters as a char key",
    value: { ...base, countByLetter: { ab: 1 } },
    accepted: false,
  },
  {
    name: "empty char key",
    value: { ...base, countByLetter: { "": 1 } },
    accepted: false,
  },
  {
    name: "enum key outside the enum",
    value: { ...base, ownerByRole: { Nobody: "x" } },
    accepted: false,
  },
  {
    name: "bad key in a nested map",
    value: { ...base, grid: { "1": { x: true } } },
    accepted: false,
  },
  {
    name: "bad outer key of a nested map",
    value: { ...base, grid: { x: { "1": true } } },
    accepted: false,
  },
  {
    name: "both bool keys",
    value: { ...base, noteByFlag: { "true": "on", "false": "off" } },
    accepted: true,
  },
  { name: "no bool keys", value: { ...base, noteByFlag: {} }, accepted: true },
  {
    name: "capitalized bool key",
    value: { ...base, noteByFlag: { True: "on" } },
    accepted: false,
  },
  {
    name: "digit as a bool key",
    value: { ...base, noteByFlag: { "1": "on" } },
    accepted: false,
  },
  {
    name: "char field of one ASCII character",
    value: { ...base, initial: "a" },
    accepted: true,
  },
  {
    name: "char field of a newline",
    value: { ...base, initial: "\n" },
    accepted: true,
  },
  {
    name: "empty char field",
    value: { ...base, initial: "" },
    accepted: false,
  },
  {
    name: "two-character char field",
    value: { ...base, initial: "ab" },
    accepted: false,
  },
  {
    name: "two astral characters as a char field",
    value: { ...base, initial: "😀😀" },
    accepted: false,
  },
  {
    name: "record link with a number key",
    value: {
      ...base,
      relatedByName: { twin: { table: "collection", key: { Number: 2 } } },
    },
    accepted: true,
  },
  {
    name: "record link with a uuid key",
    value: {
      ...base,
      relatedByName: {
        twin: {
          table: "collection",
          key: { Uuid: "0190d9df-a1b2-7c3d-8e4f-5a6b7c8d9e0f" },
        },
      },
    },
    accepted: true,
  },
  {
    name: "record link to the record itself",
    value: { ...base, relatedByName: { twin: base } },
    accepted: true,
  },
  {
    name: "record link key of no SDK kind",
    value: {
      ...base,
      relatedByName: { twin: { table: "collection", key: { Float: 2.5 } } },
    },
    accepted: false,
  },
  {
    name: "record link without a table",
    value: { ...base, relatedByName: { twin: { key: { String: "2" } } } },
    accepted: false,
  },
];

const timer = {
  id: "t1",
  limit: { secs: 3600, nanos: 0 },
  grace: null,
  laps: [],
};

const timerCases: Array<Case> = [
  { name: "every duration valid", value: timer, accepted: true },
  {
    name: "durations in a list",
    value: { ...timer, laps: [{ secs: 0, nanos: 999999999 }] },
    accepted: true,
  },
  {
    name: "duration over its bound",
    value: { ...timer, limit: { secs: 8 * 3600, nanos: 1 } },
    accepted: false,
  },
  {
    name: "duration without nanos",
    value: { ...timer, limit: { secs: 60 } },
    accepted: false,
  },
  {
    name: "duration with another key",
    value: { ...timer, limit: { secs: 60, nanos: 0, millis: 0 } },
    accepted: false,
  },
  {
    name: "duration nanos of a whole second",
    value: { ...timer, limit: { secs: 60, nanos: 1000000000 } },
    accepted: false,
  },
  {
    name: "negative duration",
    value: { ...timer, limit: { secs: -1, nanos: 0 } },
    accepted: false,
  },
  {
    name: "duration as text",
    value: { ...timer, limit: "1h" },
    accepted: false,
  },
  {
    name: "optional duration under its bound",
    value: { ...timer, grace: { secs: 30, nanos: 0 } },
    accepted: false,
  },
  {
    name: "optional duration over its bound",
    value: { ...timer, grace: { secs: 120, nanos: 0 } },
    accepted: true,
  },
];

const member = {
  id: "m1",
  displayName: "Ada",
  "zip-code": "10115",
  status: "on-hold",
};

const memberCases: Array<Case> = [
  {
    name: "renamed keys without the optional one",
    value: member,
    accepted: true,
  },
  {
    name: "the optional key present",
    value: { ...member, nickname: "ada" },
    accepted: true,
  },
  {
    name: "a renamed key under its Rust name",
    value: {
      id: "m1",
      display_name: "Ada",
      "zip-code": "10115",
      status: "active",
    },
    accepted: false,
  },
  {
    name: "a variant under its Rust name",
    value: { ...member, status: "OnHold" },
    accepted: false,
  },
  {
    name: "a renamed key failing its validator",
    value: { ...member, displayName: "A" },
    accepted: false,
  },
];

const profile = {
  name: "Ada",
  nickname: null,
  handles: ["ada"],
  visits: { home: "3" },
  label: "",
  position: ["a", 1],
  marker: null,
  status: "Active",
  title: " Lead ",
  age: "30",
};

const profileCases: Array<Case> = [
  { name: "every newtype valid", value: profile, accepted: true },
  {
    name: "an empty newtype that must be non-empty",
    value: { ...profile, name: "" },
    accepted: false,
  },
  {
    name: "an optional newtype present and empty",
    value: { ...profile, nickname: "" },
    accepted: false,
  },
  {
    name: "an optional newtype present and valid",
    value: { ...profile, nickname: "ada" },
    accepted: true,
  },
  {
    name: "a newtype over a newtype failing the inner one's validator",
    value: { ...profile, handles: ["ada", ""] },
    accepted: false,
  },
  {
    name: "a parsed newtype under its bound",
    value: { ...profile, visits: { home: "-1" } },
    accepted: false,
  },
  {
    name: "a parsed newtype that is not an integer",
    value: { ...profile, visits: { home: "three" } },
    accepted: false,
  },
  {
    name: "a parsed newtype given its decoded form",
    value: { ...profile, visits: { home: 3 } },
    accepted: false,
  },
  {
    name: "a tuple struct of the wrong length",
    value: { ...profile, position: ["a"] },
    accepted: false,
  },
  {
    name: "a unit struct that is not null",
    value: { ...profile, marker: {} },
    accepted: false,
  },
  {
    name: "a field transform emptying a newtype that must be non-empty",
    value: { ...profile, title: "   " },
    accepted: false,
  },
  {
    name: "a field parse morph into a newtype given its decoded form",
    value: { ...profile, age: 30 },
    accepted: false,
  },
  {
    name: "a field parse morph into a newtype given text that is not an integer",
    value: { ...profile, age: "thirty" },
    accepted: false,
  },
  {
    name: "a newtype over an enum outside the enum",
    value: { ...profile, status: "Gone" },
    accepted: false,
  },
];

const contactCases: Array<Case> = [
  {
    name: "a tagged newtype variant with its tag",
    value: { kind: "Phone", number: "555" },
    accepted: true,
  },
  { name: "a tagged unit variant", value: { kind: "Nobody" }, accepted: true },
  {
    name: "an untagged variant written bare",
    value: "ada@example.com",
    accepted: true,
  },
  {
    name: "an untagged struct variant written bare",
    value: { street: "Main", city: "Oslo" },
    accepted: true,
  },
  {
    name: "a tagged variant without its tag",
    value: { number: "555" },
    accepted: false,
  },
  {
    name: "an untagged variant under a tag",
    value: { kind: "Email", 0: "ada@example.com" },
    accepted: false,
  },
];

const postCases: Array<Case> = [
  {
    name: "a flattened struct's and Option's fields beside the struct's own",
    value: { title: "Hi", createdBy: "ada", lat: 1, lon: 2 },
    accepted: true,
  },
  {
    name: "a flattened Option's fields absent",
    value: { title: "Hi", createdBy: "ada" },
    accepted: true,
  },
  {
    name: "a flattened struct's field missing",
    value: { title: "Hi" },
    accepted: false,
  },
];

const settingsCases: Array<Case> = [
  {
    name: "a flattened map's keys beside the struct's own",
    value: { theme: "dark", fontSize: 3 },
    accepted: true,
  },
  {
    name: "a flattened map's value of neither the map's nor a field's type",
    value: { theme: "dark", compact: true },
    accepted: false,
  },
];

const eventCases: Array<Case> = [
  {
    name: "a flattened enum's tag and fields beside the struct's own",
    value: { at: "now", kind: "Click", x: 1 },
    accepted: true,
  },
  {
    name: "a flattened unit variant's tag",
    value: { at: "now", kind: "Close" },
    accepted: true,
  },
  {
    name: "a flattened enum's unknown tag",
    value: { at: "now", kind: "Nope" },
    accepted: false,
  },
];

const shapeCases: Array<Case> = [
  { name: "a newtype payload meeting its validator", value: { Circle: 1 }, accepted: true },
  { name: "a newtype payload failing its validator", value: { Circle: -1 }, accepted: false },
  { name: "tuple elements meeting theirs", value: { Tag: ["a", 3] }, accepted: true },
  { name: "a tuple element failing its validator", value: { Tag: ["", 3] }, accepted: false },
  { name: "an untagged member meeting its validator", value: "ABC", accepted: true },
  { name: "an untagged member failing its validator", value: "abc", accepted: false },
];

const rangeCases: Array<Case> = [
  { name: "a tuple struct's elements meeting their validators", value: [1, "x"], accepted: true },
  { name: "a tuple struct element failing its validator", value: [-1, "x"], accepted: false },
];

let failures = 0;
const check = <Decoded, Encoded>(
  cases: Array<Case>,
  ark: (value: unknown) => unknown,
  effect: Schema.Schema<Decoded, Encoded, never>,
) => {
  for (const testCase of cases) {
    const arkResult = ark(testCase.value);
    const effectResult = Schema.decodeUnknownEither(effect)(testCase.value);
    const outcomes: Array<
      { library: string; accepted: boolean; detail: string }
    > = [
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
      console.log(
        `${ok ? "ok  " : "FAIL"} ${library} ${testCase.name}: ${
          accepted ? "accepted" : "rejected"
        }${
          ok
            ? ""
            : ` (expected ${
              testCase.accepted ? "accepted" : "rejected"
            }) ${detail}`
        }`,
      );
    }
  }
};
check(cases, (value) => validator.Collection(value), Collection);
check(timerCases, (value) => durationValidator.Timer(value), Timer);
check(memberCases, (value) => memberValidator.Member(value), Member);
check(profileCases, (value) => profileValidator.Profile(value), Profile);
check(contactCases, (value) => contactValidator.Contact(value), Contact);
check(postCases, (value) => flattenValidator.Post(value), Post);
check(settingsCases, (value) => flattenValidator.Settings(value), Settings);
check(eventCases, (value) => flattenValidator.Event(value), Event);
check(shapeCases, (value) => tupleValidator.Shape(value), Shape);
check(rangeCases, (value) => tupleValidator.Range(value), Range);
console.log(
  failures === 0
    ? "all generated schema cases passed"
    : `${failures} generated schema case(s) failed`,
);
if (failures > 0) Deno.exit(1);
