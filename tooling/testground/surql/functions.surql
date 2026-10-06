-- Functions defined by schemasync after tables, so they can reference them.

DEFINE FUNCTION OVERWRITE fn::billed_item_count() {
    RETURN count(SELECT id FROM billed_item);
};
