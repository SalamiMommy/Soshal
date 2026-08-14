// ignore_for_file: invalid_use_of_internal_member
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/telemetry_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-telemetry');
  api = env.$1;
  final docsRoot = env.$2;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  Future<TelemetryService> initService() async {
    final svc = TelemetryService();
    api.stub('crateFfiTelemetryTelemetryInit', (_) => null);
    api.stubBool('crateFfiTelemetryTelemetryIsSealed', false);
    api.stub('crateFfiTelemetryTelemetryRecord', (_) => null);
    await svc.init();
    return svc;
  }

  group('TelemetryService init', () {
    test('init opens recorder under docs dir and records app-started',
        () async {
      final svc = TelemetryService();
      addTearDown(svc.dispose);
      api.stub('crateFfiTelemetryTelemetryInit', (_) => null);
      api.stubBool('crateFfiTelemetryTelemetryIsSealed', false);
      api.stub('crateFfiTelemetryTelemetryRecord', (_) => null);

      await svc.init();

      expect(svc.ready, isTrue);
      expect(svc.sealed, isFalse);
      expect(svc.lastError, isNull);
      final init = api.callsOf('crateFfiTelemetryTelemetryInit').single;
      expect(api.namedArg(init, 'path'), '$docsRoot/soshal-telemetry.bin');
      expect(api.namedArg(init, 'capacityMb'), 5);
      final rec = api.callsOf('crateFfiTelemetryTelemetryRecord').single;
      expect(api.namedArg(rec, 'kind'), 1, reason: 'RecordKind::State');
      expect(api.namedArg(rec, 'msg'), 'app-started');

      await svc.init();
      expect(api.callCount('crateFfiTelemetryTelemetryInit'), 1,
          reason: 'init is idempotent');
    });

    test('init failure sets lastError and leaves service unready', () async {
      final svc = TelemetryService();
      addTearDown(svc.dispose);
      api.stub('crateFfiTelemetryTelemetryInit',
          (_) => throw Exception('recorder busy'));

      await svc.init();

      expect(svc.ready, isFalse);
      expect(svc.lastError, contains('recorder busy'));
      svc.record('nope');
      expect(api.callCount('crateFfiTelemetryTelemetryRecord'), 0);
    });

    test('record is no-op before ready', () async {
      final svc = TelemetryService();
      addTearDown(svc.dispose);

      svc.record('early');
      svc.recordState('early-state');
      svc.recordCrash('early-crash');

      expect(api.callCount('crateFfiTelemetryTelemetryRecord'), 0);
      expect(api.callCount('crateFfiTelemetryTelemetryMarkCrash'), 0);
    });
  });

  group('TelemetryService recording', () {
    test('record and recordState pass kind/msg to the bridge', () async {
      final svc = await initService();
      addTearDown(svc.dispose);

      svc.record('hello world');
      svc.recordState('idle');

      final calls = api.callsOf('crateFfiTelemetryTelemetryRecord');
      expect(calls.length, 3, reason: 'app-started + record + state');
      expect(api.namedArg(calls[1], 'kind'), 5, reason: 'RecordKind::App');
      expect(api.namedArg(calls[1], 'msg'), 'hello world');
      expect(api.namedArg(calls[2], 'kind'), 1, reason: 'RecordKind::State');
      expect(api.namedArg(calls[2], 'msg'), 'idle');
    });

    test('record is no-op once recorder is sealed', () async {
      final svc = TelemetryService();
      addTearDown(svc.dispose);
      api.stub('crateFfiTelemetryTelemetryInit', (_) => null);
      api.stubBool('crateFfiTelemetryTelemetryIsSealed', true);
      api.stub('crateFfiTelemetryTelemetryRecord', (_) => null);

      await svc.init();

      expect(svc.ready, isTrue);
      expect(svc.sealed, isTrue);
      svc.record('dropped');
      expect(api.callCount('crateFfiTelemetryTelemetryRecord'), 0);
    });

    test('recordCrash marks reason, seals, and blocks later writes', () async {
      final svc = await initService();
      addTearDown(svc.dispose);
      api.stub('crateFfiTelemetryTelemetryMarkCrash', (_) => null);

      svc.recordCrash('fatal oom');

      expect(svc.sealed, isTrue);
      final inv = api.callsOf('crateFfiTelemetryTelemetryMarkCrash').single;
      expect(api.namedArg(inv, 'reason'), 'fatal oom');
      svc.record('after crash');
      expect(api.callCount('crateFfiTelemetryTelemetryRecord'), 1,
          reason: 'only app-started from init');
    });
  });

  group('TelemetryService export and maintenance', () {
    test('dumpEncrypted returns dump and re-reads sealed flag', () async {
      final svc = TelemetryService();
      addTearDown(svc.dispose);
      api.stub('crateFfiTelemetryTelemetryInit', (_) => null);
      var sealCalls = 0;
      api.stub('crateFfiTelemetryTelemetryIsSealed', (_) => ++sealCalls > 1);
      api.stub('crateFfiTelemetryTelemetryRecord', (_) => null);
      api.stub('crateFfiTelemetryTelemetryDumpEncrypted',
          (_) => Uint8List.fromList([7, 8, 9]));

      await svc.init();
      expect(svc.sealed, isFalse);

      final dump = svc.dumpEncrypted();

      expect(dump, [7, 8, 9]);
      expect(svc.sealed, isTrue, reason: 'dump seals the recorder');
    });

    test('dumpEncrypted failure returns null and sets lastError', () async {
      final svc = await initService();
      addTearDown(svc.dispose);
      api.stub('crateFfiTelemetryTelemetryDumpEncrypted',
          (_) => throw Exception('encrypt boom'));

      final dump = svc.dumpEncrypted();

      expect(dump, isNull);
      expect(svc.lastError, contains('encrypt boom'));
      expect(svc.sealed, isFalse);
    });

    test('readAllJson returns raw JSON; empty array fallback on error',
        () async {
      final svc = await initService();
      addTearDown(svc.dispose);
      api.stubString(
        'crateFfiTelemetryTelemetryReadAllJson',
        '[[1, 123, "boot"]]',
      );

      expect(svc.readAllJson(), '[[1, 123, "boot"]]');

      api.stub('crateFfiTelemetryTelemetryReadAllJson',
          (_) => throw Exception('read boom'));
      expect(svc.readAllJson(), '[]');
    });

    test('clear forwards to bridge and survives failure', () async {
      final svc = await initService();
      addTearDown(svc.dispose);
      api.stub('crateFfiTelemetryTelemetryClear', (_) => null);

      svc.clear();
      expect(api.callCount('crateFfiTelemetryTelemetryClear'), 1);

      api.stub('crateFfiTelemetryTelemetryClear',
          (_) => throw Exception('clear boom'));
      svc.clear();
      expect(svc.ready, isTrue, reason: 'clear failure is swallowed');
    });
  });
}