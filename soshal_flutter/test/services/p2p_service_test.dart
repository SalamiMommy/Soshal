// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/power.dart' show PowerStateDto;
import 'package:soshal_flutter/services/p2p_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

P2pPeerDto peer(int i) =>
    P2pPeerDto(pubkey: 'pk-$i', ip: '10.0.0.$i', port: 8000 + i);

P2pSwarmStatusDto status(String state, [int chunks = 3]) => P2pSwarmStatusDto(
      state: state,
      verifiedChunks: BigInt.from(chunks),
      bytesDownloaded: BigInt.from(chunks * 4096),
      failures: BigInt.from(0),
      failedHashes: const [],
    );

P2pPowerDto power(String mode, {bool paused = false}) => P2pPowerDto(
      mode: mode,
      paused: paused,
      maxParallelUploads: BigInt.from(paused ? 1 : 4),
      uploadBudgetBytesPerSec: BigInt.from(paused ? 512 : 1048576),
    );

void main() {
  final env = bootstrapTestEnv('test-p2p');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('P2pService', () {
    test('start boots LAN+QUIC servers and advertises over mDNS', () async {
      final p2p = P2pService();
      addTearDown(p2p.dispose);
      api.stubInt('crateFfiP2PP2PLanServerStart', 48001);
      api.stubInt('crateFfiP2PP2PQuicServerStart', 48002);
      api.stubBool('crateFfiP2PP2PMdnsAdvertiseStart', true);

      final port = await p2p.start(pubkey: 'pk-me');
      expect(port, 48001);
      expect(p2p.lanPort, 48001);
      expect(p2p.quicPort, 48002);
      expect(p2p.advertising, isTrue);
      expect(p2p.lastError, isNull);

      var inv = api.callsOf('crateFfiP2PP2PLanServerStart').single;
      expect(api.namedArg(inv, 'storeRoot'), '');
      inv = api.callsOf('crateFfiP2PP2PQuicServerStart').single;
      expect(api.namedArg(inv, 'storeRoot'), '');
      inv = api.callsOf('crateFfiP2PP2PMdnsAdvertiseStart').single;
      expect(api.namedArg(inv, 'pubkey'), 'pk-me');
      expect(api.namedArg(inv, 'port'), 48001);
      expect(api.namedArg(inv, 'quicPort'), 48002);
    });

    test('startBrowsing and drainPeers cap at 50', () async {
      final p2p = P2pService();
      addTearDown(p2p.dispose);
      api.stubBool('crateFfiP2PP2PMdnsBrowseStart', true);
      api.stub(
        'crateFfiP2PP2PMdnsBrowseDrain',
        (_) => List.generate(60, (i) => peer(i)),
      );

      await p2p.startBrowsing();
      expect(p2p.browsing, isTrue);

      final found = await p2p.drainPeers();
      expect(found.length, 50, reason: 'returns capped drain');
      expect(p2p.peers.length, 50, reason: 'store capped at 50');
      expect(p2p.peers.first.pubkey, 'pk-0');
      expect(p2p.peers.first.quicPort, isNull);
    });

    test('swarmDownload seeds running entry and passes args', () async {
      final p2p = P2pService();
      addTearDown(p2p.dispose);
      api.stub('crateFfiP2PP2PSwarmDownload',
          (inv) => 'dl-${api.namedArg(inv, 'outPath')}');

      final id = await p2p.swarmDownload(
        manifestJson: '{"hash":"h1"}',
        peers: const ['10.0.0.2:8001'],
        quicPorts: const [null, 9999],
        outPath: '/tmp/blob.bin',
        maxParallel: 6,
      );
      expect(id, 'dl-/tmp/blob.bin');
      expect(p2p.downloads[id]?.state, 'running');
      expect(p2p.downloads[id]?.verifiedChunks, BigInt.zero);

      final inv =
          api.callsOf('crateFfiP2PP2PSwarmDownload').single;
      expect(api.namedArg(inv, 'manifestJson'), '{"hash":"h1"}');
      expect(api.namedArg(inv, 'peersJson'), '["10.0.0.2:8001"]');
      expect(api.namedArg(inv, 'quicPortsJson'), '[null,9999]');
      expect(api.namedArg(inv, 'outPath'), '/tmp/blob.bin');
      expect(api.namedArg(inv, 'maxParallel'), BigInt.from(6));
    });

    test('swarmDownload caps active downloads at 50', () async {
      final p2p = P2pService();
      addTearDown(p2p.dispose);
      api.stub('crateFfiP2PP2PSwarmDownload',
          (inv) => 'dl-${api.namedArg(inv, 'manifestJson')}');
      final cancelled = <String>[];
      api.stub('crateFfiP2PP2PSwarmCancel', (inv) {
        cancelled.add(api.namedArg(inv, 'id'));
        return true;
      });

      for (var i = 0; i < 55; i++) {
        await p2p.swarmDownload(
          manifestJson: 'm-$i',
          peers: const ['10.0.0.2:8001'],
          quicPorts: const [null],
          outPath: '/tmp/o.bin',
          maxParallel: 4,
        );
      }
      expect(p2p.downloads.length, 50);
      expect(p2p.downloads.containsKey('dl-m-0'), isFalse,
          reason: 'oldest evicted');
      expect(p2p.downloads.containsKey('dl-m-54'), isTrue);
      // WP11: the evicted Rust worker + its mmap must be explicitly cancelled
      // (releasing the sparse file), not just dropped from the Dart map.
      expect(cancelled, contains('dl-m-0'));
    });

    test('swarmStatus keeps running, drops done; cancel removes', () async {
      final p2p = P2pService();
      addTearDown(p2p.dispose);
      api.stub('crateFfiP2PP2PSwarmDownload', (_) => 'dl-1');
      api.stub(
        'crateFfiP2PP2PSwarmPoll',
        (inv) =>
            inv.namedArguments[Symbol('id')] == 'dl-1' ? status('running') : null,
      );
      api.stubBool('crateFfiP2PP2PSwarmCancel', true);

      await p2p.swarmDownload(
        manifestJson: 'm',
        peers: const ['10.0.0.2:8001'],
        quicPorts: const [null],
        outPath: '/tmp/o.bin',
      );
      final running = await p2p.swarmStatus('dl-1');
      expect(running?.state, 'running');
      expect(running?.bytesDownloaded, BigInt.from(3 * 4096));
      expect(p2p.downloads['dl-1']?.state, 'running');

      api.stub('crateFfiP2PP2PSwarmPoll', (_) => status('done', 10));
      final done = await p2p.swarmStatus('dl-1');
      expect(done?.state, 'done');
      expect(p2p.downloads, isEmpty, reason: 'done downloads drop');

      await p2p.swarmCancel('dl-1');
      expect(p2p.downloads.isEmpty, isTrue);
    });

    test('updatePower and currentPower surface mode snapshots', () async {
      final p2p = P2pService();
      addTearDown(p2p.dispose);
      api.stub('crateFfiP2PP2PPowerUpdate', (_) => power('full'));
      api.stub('crateFfiP2PP2PPowerMode', (_) => power('paused', paused: true));

      final updated = await p2p.updatePower(
        charging: true,
        batteryPercent: 87,
        cellular: false,
        lowPowerMode: false,
      );
      expect(updated.mode, 'full');
      expect(p2p.power?.mode, 'full');
      expect(p2p.power?.maxParallelUploads, BigInt.from(4));
      final inv = api.callsOf('crateFfiP2PP2PPowerUpdate').single;
      expect(api.namedArg(inv, 'charging'), isTrue);
      expect(api.namedArg(inv, 'batteryPercent'), 87);
      expect(api.namedArg(inv, 'cellular'), isFalse);
      expect(api.namedArg(inv, 'lowPowerMode'), isFalse);

      final current = await p2p.currentPower();
      expect(current?.mode, 'paused');
      expect(current?.paused, isTrue);
      expect(p2p.power?.uploadBudgetBytesPerSec, BigInt.from(512));
    });

    test('polling samples OS power state and pushes into scheduler', () async {
      final p2p = P2pService();
      addTearDown(p2p.dispose);
      api.stub('crateFfiPowerPowerSampleOsState', (_) async {
        return PowerStateDto(
          charging: true,
          batteryPercent: 73,
          cellular: false,
          lowPowerMode: false,
        );
      });
      api.stub('crateFfiP2PP2PPowerUpdate', (_) => power('full'));

      await p2p.refreshPowerFromOs();

      expect(api.callCount('crateFfiPowerPowerSampleOsState'),
          greaterThanOrEqualTo(1));
      final inv = api.callsOf('crateFfiP2PP2PPowerUpdate').last;
      expect(api.namedArg(inv, 'charging'), isTrue);
      expect(api.namedArg(inv, 'batteryPercent'), 73);
      expect(p2p.power?.mode, 'full');
    });

    test('polling keeps last power state when sampling fails', () async {
      final p2p = P2pService();
      addTearDown(p2p.dispose);
      api.stub('crateFfiPowerPowerSampleOsState', (_) {
        throw Exception('no battery');
      });

      await p2p.refreshPowerFromOs();

      expect(api.callCount('crateFfiPowerPowerSampleOsState'),
          greaterThanOrEqualTo(1));
      expect(p2p.power, isNull);
      expect(p2p.lastError, isNull, reason: 'poll errors are swallowed');
    });

    test('fountain encode/decode roundtrip through FFI', () async {
      final p2p = P2pService();
      addTearDown(p2p.dispose);
      api.stubString(
        'crateFfiP2PP2PEncodeFountainPayload',
        '{"symbols":3,"size":42}',
      );
      api.stub('crateFfiP2PP2PDecodeFountainPayload',
          (_) => Uint8List.fromList([65, 66, 67]));

      final manifest = await p2p.encodeFountainPayload(
        data: Uint8List.fromList([1, 2, 3]),
        redundancyRatio: 1.5,
      );
      expect(manifest['symbols'], 3);
      var inv =
          api.callsOf('crateFfiP2PP2PEncodeFountainPayload').single;
      expect(api.namedArg(inv, 'data'), [1, 2, 3]);
      expect(api.namedArg(inv, 'redundancyRatio'), 1.5);

      final bytes = await p2p.decodeFountainPayload(
        manifestJson: jsonEncode(manifest),
        packetsB64Json: '["aGVsbG8="]',
      );
      expect(bytes, [65, 66, 67]);
      inv = api.callsOf('crateFfiP2PP2PDecodeFountainPayload').single;
      expect(api.namedArg(inv, 'packetsB64Json'), '["aGVsbG8="]');
    });

    test('stopAll tears down servers and clears state', () async {
      final p2p = P2pService();
      addTearDown(p2p.dispose);
      api.stubInt('crateFfiP2PP2PLanServerStart', 48001);
      api.stubInt('crateFfiP2PP2PQuicServerStart', 48002);
      api.stubBool('crateFfiP2PP2PMdnsAdvertiseStart', true);
      api.stub('crateFfiP2PP2PSwarmDownload', (_) => 'dl-1');
      api.stubBool('crateFfiP2PP2PStopAll', true);

      await p2p.start();
      await p2p.swarmDownload(
        manifestJson: 'm',
        peers: const ['10.0.0.2:8001'],
        quicPorts: const [null],
        outPath: '/tmp/o.bin',
      );
      await p2p.stopAll();

      expect(p2p.lanPort, isNull);
      expect(p2p.quicPort, isNull);
      expect(p2p.advertising, isFalse);
      expect(p2p.browsing, isFalse);
      expect(p2p.downloads, isEmpty);
      expect(p2p.peers, isEmpty);
      expect(api.callsOf('crateFfiP2PP2PStopAll').single, isNotNull);
    });

    test('start error sets lastError and rethrows', () async {
      final p2p = P2pService();
      addTearDown(p2p.dispose);
      api.stub('crateFfiP2PP2PLanServerStart',
          (_) => throw Exception('lan down'));
      await expectLater(p2p.start(), throwsException);
      expect(p2p.lastError, contains('lan down'));
    });

    test('swarmDownload error rethrows; swarmStatus swallows to null',
        () async {
      final p2p = P2pService();
      addTearDown(p2p.dispose);
      api.stub('crateFfiP2PP2PSwarmDownload',
          (_) => throw Exception('dl boom'));
      await expectLater(
        p2p.swarmDownload(
          manifestJson: 'm',
          peers: const ['10.0.0.2:8001'],
          quicPorts: const [null],
          outPath: '/tmp/o.bin',
        ),
        throwsException,
      );
      expect(p2p.lastError, contains('dl boom'));

      api.stub('crateFfiP2PP2PSwarmPoll', (_) => throw Exception('poll boom'));
      final status = await p2p.swarmStatus('dl-1');
      expect(status, isNull);
      expect(p2p.lastError, contains('poll boom'));
    });
  });
}