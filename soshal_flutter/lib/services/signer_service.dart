// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Signer Service
/// In-process key holder: lock/unlock state, OS-keychain persistence.
/// No key bytes ever cross into Dart — keyring ops exchange pubkeys only.
class SignerService extends ChangeNotifier {
  final RustLibApi _api = RustLib.instance.api;

  bool _locked = true;
  bool get locked => _locked;

  /// Whether the user explicitly locked the signer this session (Security →
  /// "Lock now"). False at boot: locked state then just means "no key loaded
  /// yet", which routes to onboarding instead of the lock screen.
  bool _userLocked = false;
  bool get userLocked => _userLocked;

  String? _pubkey;
  String? get pubkeyHex => _pubkey;

  /// Refresh lock state + active pubkey from the bridge.
  Future<void> refresh() async {
    bool locked = true;
    String? pubkey;
    try {
      pubkey = await _api.crateFfiSignerSignerPubkey();
      locked = false;
    } catch (_) {
      locked = true;
    }
    if (locked != _locked || pubkey != _pubkey) {
      _locked = locked;
      _pubkey = pubkey;
      notifyListeners();
    }
    if (!locked) _userLocked = false;
  }

  /// Active signer public key (hex). Throws when locked.
  Future<String> pubkey() async => _api.crateFfiSignerSignerPubkey();

  /// Whether the in-process signer is locked (keys zeroized).
  Future<bool> isLocked() async => _api.crateFfiSignerSignerIsLocked();

  /// Lock the session: wipes in-memory key material.
  Future<bool> lock() async {
    final ok = _api.crateFfiSignerSignerLock();
    _userLocked = true;
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
    final ok = _api.crateFfiSignerSignerUnlockFromKeyring(pubkey: pubkey);
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
}
