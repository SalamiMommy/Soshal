#!/usr/bin/env python3
"""
Automated sanitizer for flutter_rust_bridge 2.12.0 codegen corruptions.

Repairs known deterministic frb 2.12.0 corruptions in frb_generated.io.dart:
1. Strips stray spliced $allocate fragments outside class bodies.
2. Removes spliced 'typedef bool' shadowing dart:core bool.
3. Annotates unannotated 'external bool' struct fields with @ffi.Bool().
4. Ensures canonical wire types (wire_cst_list_String, mediastatus_t, etc.) are present.
"""

import re
import sys
from pathlib import Path

IO_DART = Path("soshal_flutter/lib/frb_generated.io.dart")

if not IO_DART.exists():
    print(f"Error: {IO_DART} not found", file=sys.stderr)
    sys.exit(1)

content = IO_DART.read_text(encoding="utf-8")

# 1. Remove stray spliced $allocate fragment outside class bodies
alloc_pattern = re.compile(r"^\)\s*=>\s*\$allocator<[^\n]+\n(?:[ \t]+\.\.ref\.[^\n]+\n)*\}", re.MULTILINE)
content, count_alloc = alloc_pattern.subn("", content)
if count_alloc > 0:
    print(f"Removed {count_alloc} stray spliced $allocate fragment(s)")

# 2. Remove spliced 'typedef bool' shadowing dart:core bool (handling any nested brackets)
bool_pattern = re.compile(r"^typedef bool\s*=.*?;(?:\r?\n)?", re.MULTILINE)
content, count_bool = bool_pattern.subn("", content)
if count_bool > 0:
    print(f"Purged {count_bool} stray typedef bool line(s)")

# 3. Add @ffi.Bool() annotation to struct fields declared as 'external bool <name>;'
struct_bool_pattern = re.compile(r"(?<!@ffi\.Bool\(\)\n)([ \t]+)external bool (\w+);")
content, count_struct_bool = struct_bool_pattern.subn(r"\1@ffi.Bool()\n\1external bool \2;", content)
if count_struct_bool > 0:
    print(f"Annotated {count_struct_bool} struct bool field(s) with @ffi.Bool()")

# 4. Ensure wire_cst_list_String and media typedefs are present
WIRE_LIST_STRING = """
final class wire_cst_list_String extends ffi.Struct {
  external ffi.Pointer<ffi.Pointer<wire_cst_list_prim_u_8_strict>> ptr;

  @ffi.Int32()
  external int len;

  static ffi.Pointer<wire_cst_list_String> $allocate(
    ffi.Allocator $allocator, {
    required ffi.Pointer<ffi.Pointer<wire_cst_list_prim_u_8_strict>> ptr,
    required int len,
  }) => $allocator<wire_cst_list_String>()
    ..ref.ptr = ptr
    ..ref.len = len;
}
"""

if "wire_cst_list_String" not in content:
    anchor = "const int MAX_DECODE_OUTPUT_PIXELS = 8388608;"
    if anchor in content:
        content = content.replace(anchor, anchor + "\n" + WIRE_LIST_STRING)
        print("Restored wire_cst_list_String after MAX_DECODE_OUTPUT_PIXELS")

MEDIA_TYPEDEFS = """
typedef __ssize_t = ffi.Long;
typedef Dart__ssize_t = int;
typedef aaudio_result_t = ffi.Int32;
typedef Dartaaudio_result_t = int;
typedef mediastatus_t = ffi.Int32;
typedef Dartmediastatus_t = int;
typedef ssize_t = __ssize_t;
"""

if "typedef mediastatus_t" not in content:
    anchor = "const int MAX_DECODE_OUTPUT_PIXELS = 8388608;"
    if anchor in content:
        content = content.replace(anchor, anchor + "\n" + MEDIA_TYPEDEFS)
        print("Restored media typedefs after MAX_DECODE_OUTPUT_PIXELS")

IO_DART.write_text(content, encoding="utf-8")
print("frb_generated.io.dart sanitization finished successfully.")
