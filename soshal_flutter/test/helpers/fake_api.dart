// ignore_for_file: invalid_use_of_internal_member
import 'package:soshal_flutter/frb_generated.dart';

/// Handler for a single FFI method. Receives the raw [Invocation] so stubs
/// can read named arguments.
typedef ApiHandler = dynamic Function(Invocation invocation);

/// Fake implementation of the flutter_rust_bridge API surface.
///
/// Every unimplemented member routes through [noSuchMethod] — tests register
/// handlers keyed by method name (e.g. `crateFfiFeedFeedFetchEvents`) and can
/// assert on recorded [calls]. No native library is loaded.
class FakeApi extends RustLibApi {
  final Map<Symbol, ApiHandler> handlers = {};
  final List<Invocation> calls = [];

  void stub(String method, ApiHandler handler) {
    handlers[Symbol(method)] = handler;
  }

  void stubString(String method, String result) {
    stub(method, (_) => result);
  }

  void stubStringBuilder(String method, String Function(Invocation) result) {
    stub(method, (inv) => result(inv));
  }

  void stubBool(String method, bool result) {
    stub(method, (_) => result);
  }

  void stubInt(String method, int result) {
    stub(method, (_) => result);
  }

  void stubListString(String method, List<String> result) {
    stub(method, (_) => result);
  }

  int callCount(String method) =>
      calls.where((c) => c.memberName == Symbol(method)).length;

  List<Invocation> callsOf(String method) =>
      calls.where((c) => c.memberName == Symbol(method)).toList();

  dynamic namedArg(Invocation invocation, String name) =>
      invocation.namedArguments[Symbol(name)];

  @override
  dynamic noSuchMethod(Invocation invocation) {
    calls.add(invocation);
    final handler = handlers[invocation.memberName];
    if (handler != null) return handler(invocation);
    throw UnimplementedError(
      'no FakeApi stub registered for ${invocation.memberName}',
    );
  }
}

/// Installs [api] as the singleton flutter_rust_bridge api. Must be called
/// once per test file — the RustLib singleton is per-isolate and
/// [RustLib.initMock] throws if called twice in the same isolate.
void installFakeApi(FakeApi api) {
  RustLib.initMock(api: api);
}