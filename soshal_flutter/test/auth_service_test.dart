// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/auth_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-auth');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('AuthService', () {
    test('generateKeypair stores keypair and notifies', () async {
      final auth = AuthService();
      var notified = 0;
      auth.addListener(() => notified++);

      const keypairJson = '{"public_key":"abc123","secret_key":"sec456"}';
      api.stubString('crateFfiAuthAuthGenerateKeypair', keypairJson);

      final kp = await auth.generateKeypair();
      expect(kp.publicKey, 'abc123');
      expect(kp.secretKey, 'sec456');
      expect(auth.currentKeypair, isNotNull);
      expect(auth.currentKeypair!.publicKey, 'abc123');
      expect(notified, 1);
      expect(auth.lastError, isNull);
    });

    test('generateKeypair clears previous error', () async {
      final auth = AuthService();
      api.stubString('crateFfiAuthAuthGenerateKeypair',
          '{"public_key":"pk1","secret_key":"sk1"}');

      // Simulate prior error state
      auth.setLastError(Exception('old error'), StackTrace.current);
      expect(auth.lastError, isNotNull);

      await auth.generateKeypair();
      expect(auth.lastError, isNull);
    });

    test('generateKeypair sets error on exception', () async {
      final auth = AuthService();
      api.stub('crateFfiAuthAuthGenerateKeypair', (_) {
        throw Exception('Keygen failed');
      });

      expect(
        () => auth.generateKeypair(),
        throwsException,
      );
      expect(auth.lastError, isNotNull);
      expect(auth.lastError!.contains('Keygen failed'), true);
    });

    test('generateMnemonic returns phrase', () async {
      const phrase =
          'abandon abandon abandon abandon abandon abandon abandon '
          'abandon abandon abandon abandon about';
      api.stubString('crateFfiAuthAuthGenerateMnemonic', phrase);

      final auth = AuthService();
      final result = await auth.generateMnemonic();
      expect(result, phrase);
      expect(auth.lastError, isNull);
    });

    test('generateMnemonic sets error on exception', () async {
      final auth = AuthService();
      api.stub('crateFfiAuthAuthGenerateMnemonic', (_) {
        throw Exception('Mnemonic generation failed');
      });

      expect(
        () => auth.generateMnemonic(),
        throwsException,
      );
      expect(auth.lastError, isNotNull);
    });

    test('validateMnemonic returns true for valid phrase', () async {
      api.stubBool('crateFfiAuthAuthValidateMnemonic', true);

      final auth = AuthService();
      const phrase =
          'abandon abandon abandon abandon abandon abandon abandon '
          'abandon abandon abandon abandon about';
      final result = await auth.validateMnemonic(phrase);

      expect(result, true);
      final inv = api.callsOf('crateFfiAuthAuthValidateMnemonic').single;
      expect(api.namedArg(inv, 'mnemonic'), phrase);
    });

    test('validateMnemonic returns false for invalid phrase', () async {
      api.stubBool('crateFfiAuthAuthValidateMnemonic', false);

      final auth = AuthService();
      const phrase = 'not a valid mnemonic phrase';
      final result = await auth.validateMnemonic(phrase);

      expect(result, false);
    });

    test('restoreFromMnemonic recovers keypair and notifies', () async {
      final auth = AuthService();
      var notified = 0;
      auth.addListener(() => notified++);

      const phrase =
          'abandon abandon abandon abandon abandon abandon abandon '
          'abandon abandon abandon abandon about';
      const passphrase = '';
      const keypairJson = '{"public_key":"restored_pk","secret_key":"restored_sk"}';
      api.stub('crateFfiAuthAuthRestoreFromMnemonic',
          (_) => Future.value(keypairJson));

      final kp = await auth.restoreFromMnemonic(phrase, passphrase);
      expect(kp.publicKey, 'restored_pk');
      expect(kp.secretKey, 'restored_sk');
      expect(auth.currentKeypair, kp);
      expect(notified, 1);

      final inv = api.callsOf('crateFfiAuthAuthRestoreFromMnemonic').single;
      expect(api.namedArg(inv, 'mnemonic'), phrase);
      expect(api.namedArg(inv, 'passphrase'), passphrase);
    });

    test('restoreFromMnemonic with passphrase passes correct params', () async {
      final auth = AuthService();
      const phrase = 'test phrase';
      const passphrase = 'mypass';
      const keypairJson = '{"public_key":"pk","secret_key":"sk"}';
      api.stub('crateFfiAuthAuthRestoreFromMnemonic',
          (_) => Future.value(keypairJson));

      await auth.restoreFromMnemonic(phrase, passphrase);

      final inv = api.callsOf('crateFfiAuthAuthRestoreFromMnemonic').single;
      expect(api.namedArg(inv, 'passphrase'), 'mypass');
    });

    test('getPublicKeyFromNsec chains calls correctly', () async {
      const nsec = 'nsec1...';
      const hexKey = 'abc123def456';
      const npub = 'npub1xyz789';
      api.stubString('crateFfiAuthAuthPublicKeyFromNsec', hexKey);
      api.stubString('crateFfiAuthAuthNpubEncode', npub);

      final auth = AuthService();
      final result = await auth.getPublicKeyFromNsec(nsec);

      expect(result, npub);
      expect(api.callsOf('crateFfiAuthAuthPublicKeyFromNsec').length, 1);
      expect(api.callsOf('crateFfiAuthAuthNpubEncode').length, 1);

      final hexInv = api.callsOf('crateFfiAuthAuthPublicKeyFromNsec').single;
      expect(api.namedArg(hexInv, 'nsec'), nsec);

      final npubInv = api.callsOf('crateFfiAuthAuthNpubEncode').single;
      expect(api.namedArg(npubInv, 'publicKey'), hexKey);
    });

    test('encodeNpub encodes public key', () async {
      const hexKey = 'abc123';
      const npub = 'npub1...';
      api.stubString('crateFfiAuthAuthNpubEncode', npub);

      final auth = AuthService();
      final result = await auth.encodeNpub(hexKey);

      expect(result, npub);
      final inv = api.callsOf('crateFfiAuthAuthNpubEncode').single;
      expect(api.namedArg(inv, 'publicKey'), hexKey);
    });

    test('decodeNpub decodes npub to hex', () async {
      const npub = 'npub1...';
      const hexKey = 'abc123';
      api.stubString('crateFfiAuthAuthNpubDecode', hexKey);

      final auth = AuthService();
      final result = await auth.decodeNpub(npub);

      expect(result, hexKey);
      final inv = api.callsOf('crateFfiAuthAuthNpubDecode').single;
      expect(api.namedArg(inv, 'npub'), npub);
    });

    test('KeyPair.fromJson handles both publicKey and public_key fields', () {
      final mapNew = {'public_key': 'pk1', 'secret_key': 'sk1'};
      final kp1 = KeyPair.fromJson(mapNew);
      expect(kp1.publicKey, 'pk1');
      expect(kp1.secretKey, 'sk1');

      final mapOld = {'publicKey': 'pk2', 'secretKey': 'sk2'};
      final kp2 = KeyPair.fromJson(mapOld);
      expect(kp2.publicKey, 'pk2');
      expect(kp2.secretKey, 'sk2');

      final mapMixed = {'publicKey': 'pk3', 'secret_key': 'sk3'};
      final kp3 = KeyPair.fromJson(mapMixed);
      expect(kp3.publicKey, 'pk3');
      expect(kp3.secretKey, 'sk3');
    });

    test('KeyPair.toJson emits snake_case keys', () {
      final kp = KeyPair(publicKey: 'pk', secretKey: 'sk');
      final json = kp.toJson();
      expect(json['public_key'], 'pk');
      expect(json['secret_key'], 'sk');
      expect(json.containsKey('publicKey'), false);
    });
  });
}
