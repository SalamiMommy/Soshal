// Generated callable ffi tests for events
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/events.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-events');
  final api = env.$1;

  test('eventsReminderDelete calls bool eventsReminderDelete({required String reminderId}) => RustLib.instance.api', () {
    api.stubBool('bool eventsReminderDelete({required String reminderId}) => RustLib.instance.api', true);
    final res = eventsReminderDelete(reminderId}: "x");
    expect(res, true);
    expect(api.callCount('bool eventsReminderDelete({required String reminderId}) => RustLib.instance.api'), 1);
  });

}
