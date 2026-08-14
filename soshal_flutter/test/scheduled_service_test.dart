// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/scheduled_service.dart';

import 'package:soshal_flutter/test/helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-scheduled');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('ScheduledService', () {
    test('create returns draft id and forwards args', () async {
      final scheduled = ScheduledService();
      api.stubString('crateFfiScheduledScheduledCreate', 'draft-1');

      final id = await scheduled.create(
        pubkey: 'pk-1',
        content: 'tomorrow post',
        scheduledAt: 1700000100,
        hashtags: const ['soshal'],
      );
      expect(id, 'draft-1');
      expect(scheduled.lastError, isNull);

      final inv = api.callsOf('crateFfiScheduledScheduledCreate').single;
      expect(api.namedArg(inv, 'pubkey'), 'pk-1');
      expect(api.namedArg(inv, 'content'), 'tomorrow post');
      expect(api.namedArg(inv, 'scheduledAt'), 1700000100);
      expect(api.namedArg(inv, 'hashtags'), ['soshal']);
    });

    test('list parses drafts, updates state and notifies', () async {
      final scheduled = ScheduledService();
      var notified = 0;
      scheduled.addListener(() => notified++);
      api.stubString(
        'crateFfiScheduledScheduledList',
        '[{"id":"d-1","pubkey":"pk-1","content":"later",'
        '"kind":1,"created_at":1700000001,"tags_json":"[]",'
        '"mentioned_pubkeys":["pk-2"],"mentioned_hashtags":["#x"],'
        '"sync_status":"pending","is_deleted":false,'
        '"scheduled_at":1700000100,"is_freenet_native":false}]',
      );

      final drafts = await scheduled.list('pk-1');
      final draft = drafts.single;
      expect(draft.id, 'd-1');
      expect(draft.pubkey, 'pk-1');
      expect(draft.content, 'later');
      expect(draft.kind, 1);
      expect(draft.scheduledAt, 1700000100);
      expect(draft.syncStatus, 'pending');
      expect(draft.mentionedPubkeys, ['pk-2']);
      expect(draft.mentionedHashtags, ['#x']);
      expect(scheduled.drafts.single.id, 'd-1');
      expect(scheduled.lastError, isNull);
      expect(notified, 1);

      final inv = api.callsOf('crateFfiScheduledScheduledList').single;
      expect(api.namedArg(inv, 'pubkey'), 'pk-1');
    });

    test('list with empty result clears drafts', () async {
      final scheduled = ScheduledService();
      api.stubString('crateFfiScheduledScheduledList', '[]');

      final drafts = await scheduled.list('pk-1');
      expect(drafts, isEmpty);
      expect(scheduled.drafts, isEmpty);
      expect(scheduled.lastError, isNull);
    });

    test('list with non-list payload rethrows as error', () async {
      final scheduled = ScheduledService();
      api.stubString('crateFfiScheduledScheduledList', '{"not":"a list"}');

      await expectLater(scheduled.list('pk-1'), throwsA(isA<TypeError>()));
      expect(scheduled.lastError, isNotNull);
    });

    test('delete returns soft-delete result and forwards id', () async {
      final scheduled = ScheduledService();
      api.stubBool('crateFfiScheduledScheduledDelete', true);

      final ok = await scheduled.delete('d-1');
      expect(ok, isTrue);
      final inv = api.callsOf('crateFfiScheduledScheduledDelete').single;
      expect(api.namedArg(inv, 'id'), 'd-1');
      expect(scheduled.lastError, isNull);
    });

    test('errors set lastError, notify and rethrow', () async {
      final scheduled = ScheduledService();
      var notified = 0;
      scheduled.addListener(() => notified++);
      api.stub('crateFfiScheduledScheduledCreate',
          (_) => throw Exception('db locked'));

      await expectLater(
          scheduled.create(
              pubkey: 'pk-1', content: 'x', scheduledAt: 1),
          throwsException);
      expect(scheduled.lastError, contains('db locked'));
      expect(notified, 1);

      api.stub('crateFfiScheduledScheduledDelete',
          (_) => throw Exception('row gone'));
      await expectLater(scheduled.delete('d-1'), throwsException);
      expect(scheduled.lastError, contains('row gone'));
    });
  });
}