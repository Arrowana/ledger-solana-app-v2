def type_refs:
  if type == "object" then
    ((if (.kind? == "definedTypeLinkNode") and (.name? != null) then [ .name ] else [] end)
     + (([ .[] | type_refs ] | add) // []))
  elif type == "array" then
    (([ .[] | type_refs ] | add) // [])
  else
    []
  end;

def prune_value:
  if .kind == "bytesValueNode" then
    { kind, data, encoding }
  else
    { kind }
  end;

def prune_type:
  def prune_count:
    if .kind == "fixedCountNode" then
      { kind, value }
    elif .kind == "prefixedCountNode" then
      { kind, prefix: (.prefix | prune_type) }
    else
      error("unsupported count kind: \(.kind // "null")")
    end;
  def prune_struct_field:
    { name, type: (.type | prune_type) };
  def prune_struct_body:
    { fields: (.fields | map(prune_struct_field)) };
  def prune_variant:
    if .kind == "enumEmptyVariantTypeNode" then
      ({ kind, name } + (if has("discriminator") then { discriminator } else {} end))
    elif .kind == "enumStructVariantTypeNode" then
      ({
        kind,
        name,
        struct: (.struct | prune_struct_body)
      } + (if has("discriminator") then { discriminator } else {} end))
    else
      error("unsupported enum variant kind: \(.kind // "null")")
    end;
  if .kind == "numberTypeNode" then
    { kind, format, endian }
  elif .kind == "booleanTypeNode" then
    { kind, size: (.size | prune_type) }
  elif .kind == "publicKeyTypeNode" or .kind == "bytesTypeNode" then
    { kind }
  elif .kind == "stringTypeNode" then
    { kind, encoding }
  elif .kind == "fixedSizeTypeNode" then
    { kind, size, type: (.type | prune_type) }
  elif .kind == "sizePrefixTypeNode" then
    { kind, type: (.type | prune_type), prefix: (.prefix | prune_type) }
  elif .kind == "optionTypeNode" then
    ({
      kind,
      item: (.item | prune_type),
      prefix: (.prefix | prune_type)
    } + (if has("fixed") then { fixed } else {} end))
  elif .kind == "arrayTypeNode" then
    { kind, item: (.item | prune_type), count: (.count | prune_count) }
  elif .kind == "structTypeNode" then
    ({ kind } + (. | prune_struct_body))
  elif .kind == "enumTypeNode" then
    { kind, variants: (.variants | map(prune_variant)), size: (.size | prune_type) }
  elif .kind == "definedTypeLinkNode" then
    { kind, name }
  else
    error("unsupported type kind: \(.kind // "null")")
  end;

def prune_argument:
  ({
    name,
    type: (.type | prune_type)
  } + (if has("defaultValueStrategy") then { defaultValueStrategy } else {} end)
    + (if has("defaultValue") then { defaultValue: (.defaultValue | prune_value) } else {} end));

def prune_instruction:
  { name, arguments: (.arguments | map(prune_argument)) };

def prune_defined_type:
  { name, type: (.type | prune_type) };

.program.definedTypes as $all_defs
| ($all_defs | map({ key: .name, value: . }) | from_entries) as $defs
| (.program.instructions | map(prune_instruction)) as $instructions
| ([ $instructions[] | .arguments[] | .type | type_refs[] ] | unique) as $root_refs
| def reachable($pending; $seen):
    if ($pending | length) == 0 then
      $seen
    else
      $pending[0] as $name
      | if ($seen | index($name)) then
          reachable($pending[1:]; $seen)
        else
          ($defs[$name] // null) as $def
          | if $def == null then
              reachable($pending[1:]; $seen)
            else
              reachable((($pending[1:]) + ($def.type | type_refs)); ($seen + [$name]))
            end
        end
    end;
  reachable($root_refs; []) as $keep_names
| {
    kind,
    standard,
    program: {
      name: .program.name,
      publicKey: .program.publicKey,
      version: .program.version,
      origin: .program.origin,
      instructions: $instructions,
      definedTypes: [
        $all_defs[]
        | .name as $type_name
        | select($keep_names | index($type_name))
        | prune_defined_type
      ]
    }
  }
