// ignore_for_file: invalid_use_of_internal_member
import 'package:soshal_flutter/frb_generated.dart';

/// Signer Service
/// In-process key holder: lock/unlock state, OS-keychain persistence.
/// No key bytes ever cross into Dart — keyring ops exchange pubkeys only.
class SignerService {
  final RustLibApi _api = RustLib.instance.api;

  /// Active signer public key (hex).
  Future<String> pubkey() async => _api.crateFfiSignerSignerPubkey();

  /// Whether the in-process signer is locked (keys zeroized).
  Future<bool> isLocked() async => _api.crateFfiSignerSignerIsLocked();

  /// Lock the session: wipes in-memory key material.
  Future<bool> lock() async => _api.crateFfiSignerSignerLock();

  /// Persist the signer's nsec into the OS keychain (pubkey-addressed).
  Future<bool> saveToKeyring(String pubkey) async =>
      _api.crateFfiSignerSignerSaveToKeyring(pubkey: pubkey);

  /// Restore the nsec from the OS keychain into the in-process signer.
  Future<bool> unlockFromKeyring(String pubkey) async =>
      _api.crateFfiSignerSignerUnlockFromKeyring(pubkey: pubkey);

  /// Remove the nsec from the OS keychain.
  Future<bool> removeFromKeyring(String pubkey) async =>
      _api.crateFfiSignerSignerRemoveFromKeyring(pubkey: pubkey);

  /// Sign a text message: hashes with SHA-256 then Schnorr-signs the digest.
  /// The signature is verifiable by anyone holding the signer pubkey; proves
  /// key ownership without exposing the key.
  Future<String> signText(String message) async =>
      _api.crateFfiSignerSignerSignText(message: message);
}
