// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import '../utils/format.dart';

/// Crypto tools service — diagnostics utilities backed by crypto-core FFI:
/// hashing/HMAC/HKDF, entropy, zeroize demo, PQC KEM, FROST jury signing,
/// hintless PIR, and the raw SQL console. Used by the Advanced screen.
class CryptoService extends ChangeNotifier with LastErrorMixin {
  final RustLibApi _api = RustLib.instance.api;

  /// SHA-256 of `input` as lowercase hex.
  String sha256Hex(String input) =>
      _api.crateFfiUtilUtilSha256Hex(input: input);

  /// SHA-256 of raw bytes (input is a utf8 string here), lowercase hex.
  String sha256Bytes(String input) =>
      bytesToHex(_api.crateFfiCryptoCryptoSha256(input: utf8.encode(input)));

  /// NIP-44 v2 encrypt `plaintext` to `recipientPubkey` with the unlocked
  /// signer key; returns the wire-format base64 payload.
  String nip44Encrypt(String plaintext, String recipientPubkey) =>
      _api.crateFfiSignerSignerNip44Encrypt(
          plaintext: plaintext, recipientPubkey: recipientPubkey);

  /// NIP-44 v2 decrypt `payload` from `senderPubkey` with the unlocked signer
  /// key.
  String nip44Decrypt(String payload, String senderPubkey) =>
      _api.crateFfiSignerSignerNip44Decrypt(
          payload: payload, senderPubkey: senderPubkey);

  /// Pin the calling thread to performance (true) or efficiency (false) cores.
  bool applyThreadAffinity(bool targetPerformance) =>
      _api.crateFfiUtilUtilApplyThreadAffinity(
          targetPerformance: targetPerformance);

  /// HMAC-SHA256 over `message` with `key` (both utf8), lowercase hex.
  String hmacSha256(String key, String message) =>
      bytesToHex(_api.crateFfiCryptoCryptoHmacSha256(
          key: utf8.encode(key), message: utf8.encode(message)));

  /// HKDF-SHA256 expand: derive `len` bytes from `ikm` (utf8, empty
  /// salt/info), lowercase hex.
  String hkdfExpand(String ikm, {int len = 32}) =>
      _api.crateFfiCryptoCryptoHkdfExpand(
        ikm: utf8.encode(ikm),
        salt: const <int>[],
        info: const <int>[],
        len: BigInt.from(len),
      );

  /// `len` cryptographically secure random bytes as hex (1..=65536).
  String randomBytes(int len) => _api.crateFfiCryptoCryptoRandomBytes(len: len);

  /// Zeroize demo: wipes `data` Rust-side; returns whether it completed.
  bool zeroize(String data) =>
      _api.crateFfiCryptoCryptoZeroize(data: utf8.encode(data));

  /// PQC KEM: hybrid (X25519 + ML-KEM-768) keypair. JSON `{"pk","sk"}` hex.
  Future<String> pqcKemKeygen() => _api.crateFfiCryptoCryptoPqcKemKeygen();

  /// PQC KEM: encapsulate to `recipientPk`. JSON `{"ct","ss"}` hex.
  Future<String> pqcKemEncaps(String recipientPk) =>
      _api.crateFfiCryptoCryptoPqcKemEncaps(recipientPk: recipientPk);

  /// PQC KEM: decapsulate `ciphertext` with `sk`; shared secret hex.
  Future<String> pqcKemDecaps(String ciphertext, String sk) =>
      _api.crateFfiCryptoCryptoPqcKemDecaps(ciphertext: ciphertext, sk: sk);

  /// FROST: generate t-of-n jury key shares. JSON array of key shares.
  Future<String> frostGenerateJuryKeys({
    required int threshold,
    required int totalParticipants,
    required String groupPubkey,
  }) =>
      _api.crateFfiCryptoCryptoFrostGenerateJuryKeys(
        threshold: threshold,
        totalParticipants: totalParticipants,
        groupPubkey: groupPubkey,
      );

  /// FROST: aggregate `sharesJson` (JSON array of signature shares) into a
  /// threshold Schnorr signature.
  Future<String> frostAggregateSignature({
    required String sharesJson,
    required int threshold,
    required String groupPubkey,
    required String messageHex,
  }) =>
      _api.crateFfiCryptoCryptoFrostAggregateSignature(
        sharesJson: sharesJson,
        threshold: threshold,
        groupPubkey: groupPubkey,
        messageHex: messageHex,
      );

  /// Hintless PIR: encrypted query vector for `targetIndex` in a `dimension`
  /// database.
  Future<String> pirGenerateQuery({
    required int targetIndex,
    required int dimension,
    required String clientPubkey,
  }) =>
      _api.crateFfiCryptoCryptoPirGenerateQuery(
        targetIndex: BigInt.from(targetIndex),
        dimension: BigInt.from(dimension),
        clientPubkey: clientPubkey,
      );

  /// Hintless PIR: evaluate `queryJson` over hex-encoded `recordHexList`.
  Future<String> pirEvaluateQuery({
    required String queryJson,
    required List<String> recordHexList,
  }) =>
      _api.crateFfiCryptoCryptoPirEvaluateQuery(
        queryJson: queryJson,
        recordHexList: recordHexList,
      );

  /// Raw SQL console: rows as a JSON array of objects.
  String dbQueryRaw(String sql) => _api.crateFfiDbDbQueryRaw(sql: sql);
}
