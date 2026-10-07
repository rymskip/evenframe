// A SurrealDB record id as serde writes the Rust SDK's `RecordId`, and its
// conversion to and from the JavaScript SDK's `RecordId`. The testground's
// `foreign_types.RecordId` points every TypeScript output here.
import { type } from "arktype";
import { Schema } from "effect";
import { RecordId, Uuid } from "surrealdb";

/** The record id serde writes: its table, and its key tagged by kind. */
export type RecordIdEncoded = {
  readonly table: string;
  readonly key:
    | { readonly String: string }
    | { readonly Number: number }
    | { readonly Uuid: string };
};

/** The JavaScript SDK's record id for the one serde wrote. */
export const toRecordId = (encoded: RecordIdEncoded): RecordId => {
  const key = encoded.key;
  if ("String" in key) return new RecordId(encoded.table, key.String);
  if ("Number" in key) return new RecordId(encoded.table, key.Number);
  return new RecordId(encoded.table, new Uuid(key.Uuid));
};

/** The serde form of a record id whose key is a string, number or uuid. */
export const fromRecordId = (id: RecordId): RecordIdEncoded => {
  const table = id.table.name;
  if (typeof id.id === "string") return { table, key: { String: id.id } };
  if (typeof id.id === "number") return { table, key: { Number: id.id } };
  if (id.id instanceof Uuid) return { table, key: { Uuid: id.id.toString() } };
  throw new Error(`record id ${id.toString()} has a key serde cannot write as a string, number or uuid`);
};

const encodedKey = Schema.Union(
  Schema.Struct({ String: Schema.String }),
  Schema.Struct({ Number: Schema.Number.pipe(Schema.int()) }),
  Schema.Struct({ Uuid: Schema.UUID }),
);

export const RecordIdCodec = {
  /** ArkType: the serde form. Convert a validated value with `toRecordId`. */
  ark: type({
    "+": "reject",
    table: "string",
    key: [
      [{ "+": "reject", String: "string" }, "|", { "+": "reject", Number: "number.integer" }],
      "|",
      { "+": "reject", Uuid: "string.uuid" },
    ],
  }),
  /** Effect: the serde form, decoded to the JavaScript SDK's record id. */
  schema: Schema.transform(
    Schema.Struct({ table: Schema.String, key: encodedKey }),
    Schema.instanceOf(RecordId),
    { strict: true, decode: toRecordId, encode: fromRecordId },
  ),
};
