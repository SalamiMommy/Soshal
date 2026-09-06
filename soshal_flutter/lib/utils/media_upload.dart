/// Shared media-upload helpers (pick → upload → 64-hex blob-hash validate).
///
/// Canonical home for the FilePicker/uploadMedia block copy-pasted across
/// ~9 screens. Producers store blob refs in the canonical `n<hash>` form
/// ([blobUri]); readers that only special-cased `blob://` accept both via
/// [mediaBlobHash].
library;

import 'package:file_picker/file_picker.dart';

final RegExp _hashRe = RegExp(r'^[0-9a-f]{64}$');

/// Canonical blob reference for a CAS hash: `n<hash>`.
String blobUri(String hash) => 'n$hash';

/// Extract a 64-hex CAS hash from a media reference (`blob://<hash>`,
/// `n<hash>`, or a bare hash). Null when not a blob reference.
String? mediaBlobHash(String source) {
  final trimmed = source.trim();
  if (trimmed.isEmpty) return null;
  if (trimmed.startsWith('blob://')) {
    final rest = trimmed.substring('blob://'.length);
    if (_hashRe.hasMatch(rest)) return rest;
  }
  if (trimmed.startsWith('n')) {
    final rest = trimmed.substring(1);
    if (_hashRe.hasMatch(rest)) return rest;
  }
  if (_hashRe.hasMatch(trimmed)) return trimmed;
  return null;
}

/// Picked+uploaded blob descriptor returned by [pickAndUploadMedia].
typedef MediaUpload = ({String path, String hash, String uri, int size});

/// Pick a file via the system picker; returns its path or null when the
/// user cancels.
Future<String?> pickMediaPath({FileType type = FileType.image}) async {
  final picked = await FilePicker.pickFile(type: type);
  return picked?.path;
}

/// Upload [path] via [upload] and validate the manifest `blob_hash` is a
/// 64-hex string. Returns the bare hash. Throws [Exception] with
/// [errorMessage] on a bad manifest — the contract every legacy call site
/// relied on (call sites keep their exact user-visible message).
Future<String> uploadMediaBlob(
  Future<Map<String, dynamic>> Function(String path) upload, {
  required String path,
  String errorMessage = 'Upload failed (bad manifest)',
}) async {
  final manifest = await upload(path);
  return validatedBlobHash(manifest, errorMessage);
}

/// Pick a file, upload it, and return its blob descriptor. Null when the
/// picker is cancelled. [errorMessage] is surfaced as-is on bad manifests.
Future<MediaUpload?> pickAndUploadMedia(
  Future<Map<String, dynamic>> Function(String path) upload, {
  FileType type = FileType.image,
  String errorMessage = 'Upload failed (bad manifest)',
}) async {
  final path = await pickMediaPath(type: type);
  if (path == null) return null;
  final manifest = await upload(path);
  final hash = validatedBlobHash(manifest, errorMessage);
  final size = (manifest['total_size'] as num?)?.toInt() ?? 0;
  return (path: path, hash: hash, uri: blobUri(hash), size: size);
}

/// Validate an upload manifest's `blob_hash` is a 64-hex string. Throws
/// [Exception] with [errorMessage] otherwise; returns the hash when valid.
String validatedBlobHash(
  Map<String, dynamic> manifest,
  String errorMessage,
) {
  final hash = manifest['blob_hash'] as String? ?? '';
  if (hash.length != 64) {
    throw Exception(errorMessage);
  }
  return hash;
}