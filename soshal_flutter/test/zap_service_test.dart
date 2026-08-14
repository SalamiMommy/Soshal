// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/zap_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-zap');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('ZapService', () {
    test('connect passes nwcUri and reflects connected state', () async {
      final zap = ZapService();
      api.stubBool('crateFfiZapZapConnectNwc', true);
      api.stubString('crateFfiZapZapGetNwcStatus', 'connected');
      api.stubString('crateFfiZapZapGetNwcPubkey', 'npub1zapper');

      expect(await zap.connect('nostr+walletconnect://lud16'), isTrue);
      final inv = api.callsOf('crateFfiZapZapConnectNwc').single;
      expect(api.namedArg(inv, 'nwcUri'), 'nostr+walletconnect://lud16');
      expect(zap.isConnected, isTrue);
      expect(zap.nwcPubkey, 'npub1zapper');
    });

    test('connect failure keeps disconnected state and clears error',
        () async {
      final zap = ZapService();
      api.stubBool('crateFfiZapZapConnectNwc', false);

      expect(await zap.connect('nostr+walletconnect://bad'), isFalse);
      expect(zap.isConnected, isFalse);
      expect(zap.lastError, isNull);
    });

    test('disconnect transitions to disconnected and clears pubkey',
        () async {
      final zap = ZapService();
      api.stubBool('crateFfiZapZapDisconnectNwc', true);

      expect(await zap.disconnect(), isTrue);
      expect(zap.nwcStatus, 'disconnected');
      expect(zap.nwcPubkey, isNull);
      expect(zap.isConnected, isFalse);
    });

    test('refreshStatus surfaces status and pubkey from FFI', () async {
      final zap = ZapService();
      api.stubString('crateFfiZapZapGetNwcStatus', 'connected');
      api.stubString('crateFfiZapZapGetNwcPubkey', 'npub1zapper');

      await zap.refreshStatus();
      expect(zap.nwcStatus, 'connected');
      expect(zap.nwcPubkey, 'npub1zapper');
      expect(zap.isConnected, isTrue);
    });

    test('honest stub: connect throw surfaces error, no fake state',
        () async {
      final zap = ZapService();
      api.stub('crateFfiZapZapConnectNwc',
          (_) => throw Exception('backend err'));
      api.stubString('crateFfiZapZapGetNwcStatus', 'connected');

      await expectLater(zap.connect('nostr+walletconnect://x'), throwsException);
      expect(zap.lastError, contains('backend err'));
      expect(zap.isConnected, isFalse,
          reason: 'never fabricate a connected state');
    });

    test('honest stub: fetchTotalMsat throw surfaces error, no fake total',
        () async {
      final zap = ZapService();
      api.stub('crateFfiZapZapGetTotalMsat',
          (_) => throw Exception('nwc down'));

      await expectLater(zap.fetchTotalMsat('ev-1'), throwsException);
      expect(zap.lastError, contains('nwc down'));
      expect(zap.totalMsat, 0,
          reason: 'never fabricate a zap total');
    });

    test('honest stub: fetchReceipts throw surfaces error, no fake receipts',
        () async {
      final zap = ZapService();
      api.stub('crateFfiZapZapFetchReceipts',
          (_) => throw Exception('nwc down'));

      await expectLater(zap.fetchReceipts('ev-1'), throwsException);
      expect(zap.lastError, contains('nwc down'));
      expect(zap.receipts, isEmpty,
          reason: 'never fabricate zap receipts');
    });

    test('fetchTotalMsat returns parsed total and passes eventId', () async {
      final zap = ZapService();
      api.stubInt('crateFfiZapZapGetTotalMsat', 21000);

      expect(await zap.fetchTotalMsat('ev-1'), 21000);
      expect(zap.totalMsat, 21000);
      final inv = api.callsOf('crateFfiZapZapGetTotalMsat').single;
      expect(api.namedArg(inv, 'eventId'), 'ev-1');
    });

    test('fetchReceipts parses JSON and passes eventId + limit', () async {
      final zap = ZapService();
      api.stubString(
        'crateFfiZapZapFetchReceipts',
        '[{"id":"r1","event_id":"ev-1","zapper_pubkey":"pk-1",'
        '"amount_msat":1000,"created_at":1700000000}]',
      );

      final receipts = await zap.fetchReceipts('ev-1', limit: 5);
      expect(receipts.single.id, 'r1');
      expect(receipts.single.eventId, 'ev-1');
      expect(receipts.single.zapperPubkey, 'pk-1');
      expect(receipts.single.amountMsat, 1000);
      expect(receipts.single.createdAt, 1700000000);
      final inv = api.callsOf('crateFfiZapZapFetchReceipts').single;
      expect(api.namedArg(inv, 'eventId'), 'ev-1');
      expect(api.namedArg(inv, 'limit'), 5);
    });

    test('parseLnurl returns metadata JSON and passes lnurl', () async {
      final zap = ZapService();
      api.stubString(
        'crateFfiZapZapParseLnurlMetadata',
        '{"minSendable":1000,"maxSendable":100000}',
      );

      expect(await zap.parseLnurl('lnurl1abc'), contains('minSendable'));
      final inv = api.callsOf('crateFfiZapZapParseLnurlMetadata').single;
      expect(api.namedArg(inv, 'lnurl'), 'lnurl1abc');
    });

    test('parseLnurl error surfaces in lastError and rethrows', () async {
      final zap = ZapService();
      api.stub('crateFfiZapZapParseLnurlMetadata',
          (_) => throw Exception('bad lnurl'));

      await expectLater(zap.parseLnurl('lnurl1bad'), throwsException);
      expect(zap.lastError, contains('bad lnurl'));
    });
  });
}