// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Authentication Service
/// Handles key generation, login, and session management
class AuthService extends ChangeNotifier with LastErrorMixin {
  KeyPair? _currentKeypair;

  KeyPair? get currentKeypair => _currentKeypair;

  /// Generate a new keypair
  Future<KeyPair> generateKeypair() async {
    try {
      final json = RustLib.instance.api.crateFfiAuthAuthGenerateKeypair();
      _currentKeypair = _decode(json);
      clearLastError();
      notifyListeners();
      return _currentKeypair!;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Generate a new BIP-39 mnemonic phrase
  Future<String> generateMnemonic() async {
    try {
      final mnemonic = RustLib.instance.api.crateFfiAuthAuthGenerateMnemonic();
      clearLastError();
      return mnemonic;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Validate a BIP-39 mnemonic phrase
  Future<bool> validateMnemonic(String mnemonic) async {
    try {
      return RustLib.instance.api
          .crateFfiAuthAuthValidateMnemonic(mnemonic: mnemonic);
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Restore a keypair from a BIP-39 mnemonic
  Future<KeyPair> restoreFromMnemonic(
      String mnemonic, String passphrase) async {
    try {
      final json =
          await RustLib.instance.api.crateFfiAuthAuthRestoreFromMnemonic(
        mnemonic: mnemonic,
        passphrase: passphrase,
      );
      _currentKeypair = _decode(json);
      clearLastError();
      notifyListeners();
      return _currentKeypair!;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Get public key from nsec, returned as npub
  Future<String> getPublicKeyFromNsec(String nsec) async {
    try {
      final hex =
          RustLib.instance.api.crateFfiAuthAuthPublicKeyFromNsec(nsec: nsec);
      return RustLib.instance.api.crateFfiAuthAuthNpubEncode(publicKey: hex);
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Encode public key as npub
  Future<String> encodeNpub(String publicKey) async {
    try {
      return RustLib.instance.api
          .crateFfiAuthAuthNpubEncode(publicKey: publicKey);
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Decode npub back to hex public key
  Future<String> decodeNpub(String npub) async {
    try {
      return RustLib.instance.api.crateFfiAuthAuthNpubDecode(npub: npub);
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Handle a nostr protocol request
  Future<void> handleNostrProtocolRequest(
      {required String scheme,
      required String host,
      required String path}) async {
    try {
      await RustLib.instance.api.crateFfiProtocolHandlerProtocolHandleRequest(
        scheme: scheme,
        host: host,
        path: path,
      );
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Build an in-process signer from an nsec (diagnostics only); returns
  /// the derived hex pubkey.
  Future<String> inProcessSignerPubkey(String nsec) async {
    try {
      final pubkey = RustLib.instance.api
          .crateFfiIdentityIdentityInProcessSigner(nsec: nsec);
      clearLastError();
      return pubkey;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  static KeyPair _decode(String json) {
    final map = jsonDecode(json) as Map<String, dynamic>;
    return KeyPair.fromJson(map);
  }
}

/// Key pair produced by the Rust keygen/restore calls.
class KeyPair {
  final String publicKey;
  final String secretKey;

  KeyPair({
    required this.publicKey,
    required this.secretKey,
  });

  factory KeyPair.fromJson(Map<String, dynamic> json) {
    return KeyPair(
      publicKey:
          json['publicKey'] as String? ?? json['public_key'] as String? ?? '',
      secretKey:
          json['secretKey'] as String? ?? json['secret_key'] as String? ?? '',
    );
  }

  Map<String, dynamic> toJson() {
    return {
      'public_key': publicKey,
      'secret_key': secretKey,
    };
  }
}
