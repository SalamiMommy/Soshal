import '../utils/json_ext.dart';
// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Authentication Service
/// Handles key generation, login, and session management
class AuthService extends ChangeNotifier with LastErrorMixin, ServiceGuard {
  KeyPair? _currentKeypair;

  KeyPair? get currentKeypair => _currentKeypair;

  /// Generate a new keypair
  Future<KeyPair> generateKeypair() => guard(() {
        final json = RustLib.instance.api.crateFfiAuthAuthGenerateKeypair();
        _currentKeypair = _decode(json);
        return _currentKeypair!;
      });

  /// Drop the transient onboarding nsec from the Dart heap. The keypair
  /// keeps its public key; only the secret is cleared. Called once the
  /// backup-display dialog closes — the secret is never needed afterward.
  /// The Rust signer never returns a secret across FFI, so this is a no-op
  /// guard for legacy responses that may have carried one.
  void clearSecretKey() {
    final kp = _currentKeypair;
    if (kp == null) return;
    _currentKeypair = KeyPair(publicKey: kp.publicKey);
    notifyListeners();
  }

  Future<String> generateMnemonic() => guard(() {
        return RustLib.instance.api.crateFfiAuthAuthGenerateMnemonic();
      }, clearOnSuccess: false, notifyOnSuccess: false);

  Future<bool> validateMnemonic(String mnemonic) => guard(() {
        return RustLib.instance.api
            .crateFfiAuthAuthValidateMnemonic(mnemonic: mnemonic);
      }, clearOnSuccess: false, notifyOnSuccess: false);

  Future<KeyPair> restoreFromMnemonic(String mnemonic, String passphrase) =>
      guard(() async {
        final json =
            await RustLib.instance.api.crateFfiAuthAuthRestoreFromMnemonic(
          mnemonic: mnemonic,
          passphrase: passphrase,
        );
        _currentKeypair = _decode(json);
        return _currentKeypair!;
      });

  Future<String> getPublicKeyFromNsec(String nsec) => guard(() {
        final hex =
            RustLib.instance.api.crateFfiAuthAuthPublicKeyFromNsec(nsec: nsec);
        return RustLib.instance.api.crateFfiAuthAuthNpubEncode(publicKey: hex);
      }, clearOnSuccess: false, notifyOnSuccess: false);

  Future<String> encodeNpub(String publicKey) => guard(() {
        return RustLib.instance.api
            .crateFfiAuthAuthNpubEncode(publicKey: publicKey);
      }, clearOnSuccess: false, notifyOnSuccess: false);

  Future<String> decodeNpub(String npub) => guard(() {
        return RustLib.instance.api.crateFfiAuthAuthNpubDecode(npub: npub);
      }, clearOnSuccess: false, notifyOnSuccess: false);

  Future<void> handleNostrProtocolRequest(
          {required String scheme,
          required String host,
          required String path}) =>
      guard(() async {
        await RustLib.instance.api.crateFfiProtocolHandlerProtocolHandleRequest(
          scheme: scheme,
          host: host,
          path: path,
        );
      }, clearOnSuccess: false, notifyOnSuccess: false);

  Future<String> inProcessSignerPubkey(String nsec) => guard(() {
        return RustLib.instance.api
            .crateFfiIdentityIdentityInProcessSigner(nsec: nsec);
      }, notifyOnSuccess: false);

  static KeyPair _decode(String json) {
    final map = jsonDecode(json) as Map<String, dynamic>;
    return KeyPair.fromJson(map);
  }
}

/// Key pair produced by the Rust keygen/restore calls.
/// No secret key is ever stored: the Rust signer holds the secret
/// in-process and never returns it across FFI, and backup goes through the
/// BIP-39 mnemonic. Any `secretKey`/`secret_key` present in a decoded
/// response is deliberately dropped.
class KeyPair {
  final String publicKey;

  KeyPair({required this.publicKey});

  factory KeyPair.fromJson(Map<String, dynamic> json) {
    return KeyPair(
      publicKey: json.strOrNull('publicKey') ?? json.strOf('public_key'),
    );
  }

  Map<String, dynamic> toJson() {
    return {
      'public_key': publicKey,
    };
  }
}
