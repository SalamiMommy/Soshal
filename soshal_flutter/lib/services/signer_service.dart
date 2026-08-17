// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'crypto_service.dart';

/// Signer Service
/// In-process key holder: lock/unlock state, OS-keychain persistence.
/// No key bytes ever cross into Dart — keyring ops exchange pubkeys only.
class SignerService extends ChangeNotifier {
  final RustLibApi _api = RustLib.instance.api;

  bool _locked = true;
  bool get locked => _locked;

  String? _pubkey;
  String? get pubkeyHex => _pubkey;

  /// Refresh lock state + active pubkey from the bridge.
  Future<void> refresh() async {
    bool locked = true;
    String? pubkey;
    try {
      pubkey = _api.crateFfiSignerSignerPubkey();
      locked = false;
    } catch (_) {
      locked = true;
    }
    if (locked != _locked || pubkey != _pubkey) {
      _locked = locked;
      _pubkey = pubkey;
      notifyListeners();
    }
  }

  /// Active signer public key (hex). Throws when locked.
  Future<String> pubkey() async => _api.crateFfiSignerSignerPubkey();

  /// Whether the in-process signer is locked (keys zeroized).
  Future<bool> isLocked() async => _api.crateFfiSignerSignerIsLocked();

  /// Lock the session: wipes in-memory key material.
  Future<bool> lock() async {
    final ok = _api.crateFfiSignerSignerLock();
    await refresh();
    return ok;
  }

  /// Unlock the signer with an nsec or hex secret key.
  Future<String> unlock(String secret) async {
    final pk = _api.crateFfiSignerSignerUnlock(secret: secret);
    await refresh();
    return pk;
  }

  /// Persist the signer's nsec into the OS keychain (pubkey-addressed).
  Future<bool> saveToKeyring(String pubkey) async =>
      _api.crateFfiSignerSignerSaveToKeyring(pubkey: pubkey);

  /// Restore the nsec from the OS keychain into the in-process signer.
  Future<bool> unlockFromKeyring(String pubkey) async {
    final ok = await _api.crateFfiSignerSignerUnlockFromKeyring(pubkey: pubkey);
    await refresh();
    return ok;
  }

  /// Remove the nsec from the OS keychain.
  Future<bool> removeFromKeyring(String pubkey) async =>
      _api.crateFfiSignerSignerRemoveFromKeyring(pubkey: pubkey);

  /// Sign a text message: hashes with SHA-256 then Schnorr-signs the digest.
  /// The signature is verifiable by anyone holding the signer pubkey; proves
  /// key ownership without exposing the key.
  Future<String> signText(String message) async =>
      _api.crateFfiSignerSignerSignText(message: message);

  /// Schnorr-sign the SHA-256 digest of `text` (digest computed Dart-side,
  /// signed Rust-side); returns the 64-byte signature as hex.
  Future<String> schnorrSign(String text) async {
    final digest = _api.crateFfiUtilUtilSha256Hex(input: text);
    return _api.crateFfiSignerSignerSchnorrSign(messageHex: digest);
  }

  /// Sign a NIP-59-style unsigned event JSON (`pubkey`, `created_at`,
  /// `kind`, `tags`, `content`; `id` optional); returns the fully signed
  /// event JSON including `id` and `sig`.
  Future<String> signUnsigned(String eventJson) async =>
      _api.crateFfiSignerSignerSignUnsigned(eventJson: eventJson);

  /// NIP-44 v2 encrypt `plaintext` to `recipientPubkey` with the unlocked
  /// key; returns the wire-format base64 payload (`2 ‖ nonce ‖ ct ‖ mac`).
  Future<String> nip44Encrypt(String plaintext, String recipientPubkey) async =>
      CryptoService().nip44Encrypt(plaintext, recipientPubkey);

  /// NIP-44 v2 decrypt `ciphertext` (from `senderPubkey`) with the unlocked
  /// key.
  Future<String> nip44Decrypt(String ciphertext, String senderPubkey) async =>
      CryptoService().nip44Decrypt(ciphertext, senderPubkey);
}
