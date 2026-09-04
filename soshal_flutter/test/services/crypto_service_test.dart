// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/crypto_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-crypto');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('CryptoService', () {
    test('sha256Hex forwards input and returns digest', () {
      api.stubString('crateFfiUtilUtilSha256Hex', 'abc123');
      final crypto = CryptoService();

      expect(crypto.sha256Hex('hello'), 'abc123');
      final inv = api.callsOf('crateFfiUtilUtilSha256Hex').single;
      expect(api.namedArg(inv, 'input'), 'hello');
    });

    test('sha256Bytes encodes input utf8 and hexes returned bytes', () {
      api.stub('crateFfiCryptoCryptoSha256',
          (_) => Uint8List.fromList([0x00, 0x0f, 0xff, 0x10]));
      final crypto = CryptoService();

      expect(crypto.sha256Bytes('héllo'), '000fff10');
      final inv = api.callsOf('crateFfiCryptoCryptoSha256').single;
      expect(api.namedArg(inv, 'input'), utf8.encode('héllo'));
    });

    test('sha256Bytes empty input forwards empty byte list', () {
      api.stub('crateFfiCryptoCryptoSha256', (_) => Uint8List(0));
      final crypto = CryptoService();

      expect(crypto.sha256Bytes(''), '');
      final inv = api.callsOf('crateFfiCryptoCryptoSha256').single;
      expect(api.namedArg(inv, 'input'), isEmpty);
    });

    test('nip44Encrypt forwards plaintext and recipient pubkey', () {
      api.stubString('crateFfiSignerSignerNip44Encrypt', 'wire-payload');
      final crypto = CryptoService();

      expect(crypto.nip44Encrypt('secret', 'pk-2'), 'wire-payload');
      final inv =
          api.callsOf('crateFfiSignerSignerNip44Encrypt').single;
      expect(api.namedArg(inv, 'plaintext'), 'secret');
      expect(api.namedArg(inv, 'recipientPubkey'), 'pk-2');
    });

    test('nip44Decrypt forwards payload and sender pubkey', () {
      api.stubString('crateFfiSignerSignerNip44Decrypt', 'plain');
      final crypto = CryptoService();

      expect(crypto.nip44Decrypt('wire-payload', 'pk-1'), 'plain');
      final inv =
          api.callsOf('crateFfiSignerSignerNip44Decrypt').single;
      expect(api.namedArg(inv, 'payload'), 'wire-payload');
      expect(api.namedArg(inv, 'senderPubkey'), 'pk-1');
    });

    test('applyThreadAffinity forwards target and returns result', () {
      api.stubBool('crateFfiUtilUtilApplyThreadAffinity', true);
      final crypto = CryptoService();

      expect(crypto.applyThreadAffinity(true), isTrue);
      final inv =
          api.callsOf('crateFfiUtilUtilApplyThreadAffinity').single;
      expect(api.namedArg(inv, 'targetPerformance'), isTrue);
    });

    test('hmacSha256 encodes key/message utf8 and hexes result', () {
      api.stub('crateFfiCryptoCryptoHmacSha256',
          (_) => Uint8List.fromList([0xde, 0xad]));
      final crypto = CryptoService();

      expect(crypto.hmacSha256('key', 'msg'), 'dead');
      final inv = api.callsOf('crateFfiCryptoCryptoHmacSha256').single;
      expect(api.namedArg(inv, 'key'), utf8.encode('key'));
      expect(api.namedArg(inv, 'message'), utf8.encode('msg'));
    });

    test('hkdfExpand forwards len as BigInt, empty salt/info, default 32',
        () {
      api.stubString('crateFfiCryptoCryptoHkdfExpand', 'e0e1');
      final crypto = CryptoService();

      expect(crypto.hkdfExpand('ikm'), 'e0e1');
      var inv = api.callsOf('crateFfiCryptoCryptoHkdfExpand').single;
      expect(api.namedArg(inv, 'ikm'), utf8.encode('ikm'));
      expect(api.namedArg(inv, 'salt'), isEmpty);
      expect(api.namedArg(inv, 'info'), isEmpty);
      expect(api.namedArg(inv, 'len'), BigInt.from(32));

      expect(crypto.hkdfExpand('ikm', len: 64), 'e0e1');
      inv = api.callsOf('crateFfiCryptoCryptoHkdfExpand').last;
      expect(api.namedArg(inv, 'len'), BigInt.from(64));
    });

    test('randomBytes forwards requested length', () {
      api.stubString('crateFfiCryptoCryptoRandomBytes', 'ff00');
      final crypto = CryptoService();

      expect(crypto.randomBytes(16), 'ff00');
      final inv = api.callsOf('crateFfiCryptoCryptoRandomBytes').single;
      expect(api.namedArg(inv, 'len'), 16);
    });

    test('zeroize forwards data utf8 and returns completion', () {
      api.stubBool('crateFfiCryptoCryptoZeroize', true);
      final crypto = CryptoService();

      expect(crypto.zeroize('sensitive'), isTrue);
      final inv = api.callsOf('crateFfiCryptoCryptoZeroize').single;
      expect(api.namedArg(inv, 'data'), utf8.encode('sensitive'));
    });

    test('pqcKemKeygen returns keypair json', () async {
      api.stubString(
          'crateFfiCryptoCryptoPqcKemKeygen', '{"pk":"a","sk":"b"}');
      final crypto = CryptoService();

      expect(await crypto.pqcKemKeygen(), '{"pk":"a","sk":"b"}');
    });

    test('pqcKemEncaps forwards recipient pk', () async {
      api.stubString('crateFfiCryptoCryptoPqcKemEncaps', '{"ct":"c"}');
      final crypto = CryptoService();

      expect(await crypto.pqcKemEncaps('recipient-pk'), '{"ct":"c"}');
      final inv = api.callsOf('crateFfiCryptoCryptoPqcKemEncaps').single;
      expect(api.namedArg(inv, 'recipientPk'), 'recipient-pk');
    });

    test('pqcKemDecaps forwards ciphertext and sk', () async {
      api.stubString('crateFfiCryptoCryptoPqcKemDecaps', 'ss-hex');
      final crypto = CryptoService();

      expect(await crypto.pqcKemDecaps('ct-hex', 'sk-hex'), 'ss-hex');
      final inv = api.callsOf('crateFfiCryptoCryptoPqcKemDecaps').single;
      expect(api.namedArg(inv, 'ciphertext'), 'ct-hex');
      expect(api.namedArg(inv, 'sk'), 'sk-hex');
    });

    test('frostGenerateJuryKeys forwards threshold/participants/pubkey',
        () async {
      api.stubString('crateFfiCryptoCryptoFrostGenerateJuryKeys',
          '[{"share":"s1"}]');
      final crypto = CryptoService();

      expect(
        await crypto.frostGenerateJuryKeys(
            threshold: 2, totalParticipants: 3, groupPubkey: 'gp'),
        '[{"share":"s1"}]',
      );
      final inv =
          api.callsOf('crateFfiCryptoCryptoFrostGenerateJuryKeys').single;
      expect(api.namedArg(inv, 'threshold'), 2);
      expect(api.namedArg(inv, 'totalParticipants'), 3);
      expect(api.namedArg(inv, 'groupPubkey'), 'gp');
    });

    test('frostAggregateSignature forwards shares/threshold/pubkey/message',
        () async {
      api.stubString(
          'crateFfiCryptoCryptoFrostAggregateSignature', 'sig-hex');
      final crypto = CryptoService();

      expect(
        await crypto.frostAggregateSignature(
            sharesJson: '[{"s":"a"}]',
            threshold: 2,
            groupPubkey: 'gp',
            messageHex: 'dead'),
        'sig-hex',
      );
      final inv = api
          .callsOf('crateFfiCryptoCryptoFrostAggregateSignature')
          .single;
      expect(api.namedArg(inv, 'sharesJson'), '[{"s":"a"}]');
      expect(api.namedArg(inv, 'threshold'), 2);
      expect(api.namedArg(inv, 'groupPubkey'), 'gp');
      expect(api.namedArg(inv, 'messageHex'), 'dead');
    });

    test('pirGenerateQuery forwards index/dimension as BigInt and pubkey',
        () async {
      api.stubString('crateFfiCryptoCryptoPirGenerateQuery', 'query-json');
      final crypto = CryptoService();

      expect(
        await crypto.pirGenerateQuery(
            targetIndex: 7, dimension: 1024, clientPubkey: 'cp'),
        'query-json',
      );
      final inv =
          api.callsOf('crateFfiCryptoCryptoPirGenerateQuery').single;
      expect(api.namedArg(inv, 'targetIndex'), BigInt.from(7));
      expect(api.namedArg(inv, 'dimension'), BigInt.from(1024));
      expect(api.namedArg(inv, 'clientPubkey'), 'cp');
    });

    test('pirEvaluateQuery forwards query and record list', () async {
      api.stubString('crateFfiCryptoCryptoPirEvaluateQuery', 'row-json');
      final crypto = CryptoService();

      expect(
        await crypto.pirEvaluateQuery(
            queryJson: 'q', recordHexList: ['aa', 'bb']),
        'row-json',
      );
      final inv =
          api.callsOf('crateFfiCryptoCryptoPirEvaluateQuery').single;
      expect(api.namedArg(inv, 'queryJson'), 'q');
      expect(api.namedArg(inv, 'recordHexList'), ['aa', 'bb']);
    });

    test('pirEvaluateQuery forwards empty record list', () async {
      api.stubString('crateFfiCryptoCryptoPirEvaluateQuery', '[]');
      final crypto = CryptoService();

      expect(
        await crypto.pirEvaluateQuery(queryJson: 'q', recordHexList: []),
        '[]',
      );
    });

    test('dbQueryRaw forwards sql and returns rows json', () {
      api.stubString('crateFfiDbDbQueryRaw', '[{"n":1}]');
      final crypto = CryptoService();

      expect(crypto.dbQueryRaw('SELECT 1'), '[{"n":1}]');
      final inv = api.callsOf('crateFfiDbDbQueryRaw').single;
      expect(api.namedArg(inv, 'sql'), 'SELECT 1');
    });

    test('sync method ffi error propagates, lastError stays null', () {
      api.stub('crateFfiUtilUtilSha256Hex', (_) => throw Exception('boom'));
      final crypto = CryptoService();

      expect(() => crypto.sha256Hex('x'), throwsException);
      expect(crypto.lastError, isNull);
    });

    test('sync bool/bytes method ffi error propagates', () {
      api.stub('crateFfiCryptoCryptoZeroize',
          (_) => throw Exception('zeroize fail'));
      final crypto = CryptoService();

      expect(() => crypto.zeroize('x'), throwsException);
      expect(crypto.lastError, isNull);
    });

    test('async method ffi sync-throw propagates as failed future', () async {
      api.stub('crateFfiCryptoCryptoPqcKemKeygen',
          (_) => throw Exception('keygen fail'));
      final crypto = CryptoService();

      await expectLater(() => crypto.pqcKemKeygen(), throwsException);
      expect(crypto.lastError, isNull);
    });

    test('async method ffi failed future propagates', () async {
      api.stub('crateFfiCryptoCryptoPqcKemEncaps',
          (_) => Future.error(Exception('encaps fail')));
      final crypto = CryptoService();

      await expectLater(
          () => crypto.pqcKemEncaps('pk'), throwsException);
      expect(crypto.lastError, isNull);
    });

    test('frost failure propagates', () async {
      api.stub('crateFfiCryptoCryptoFrostGenerateJuryKeys',
          (_) => Future.error(Exception('frost fail')));
      final crypto = CryptoService();

      await expectLater(
        () => crypto.frostGenerateJuryKeys(
            threshold: 2, totalParticipants: 3, groupPubkey: 'gp'),
        throwsException,
      );
    });

    test('pir failure propagates', () async {
      api.stub('crateFfiCryptoCryptoPirEvaluateQuery',
          (_) => Future.error(Exception('pir fail')));
      final crypto = CryptoService();

      await expectLater(
        () => crypto.pirEvaluateQuery(
            queryJson: 'q', recordHexList: ['aa']),
        throwsException,
      );
    });
  });
}