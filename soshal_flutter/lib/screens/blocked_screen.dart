import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/messaging_service.dart';
import '../services/session_service.dart';

/// Blocked users screen: list + unblock.
class BlockedScreen extends StatefulWidget {
  /// Blocked users screen.
  const BlockedScreen({super.key});

  @override
  State<BlockedScreen> createState() => _BlockedScreenState();
}

class _BlockedScreenState extends State<BlockedScreen> {
  List<String> _blocked = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      var session = context.read<SessionService>();
      var api = context.read<IdentityService>();
      final pubkey = session.activePubkey;
      if (pubkey != null) {
        _blocked = await api.getBlockedUsers(pubkey);
      }
    } catch (e) {
      debugPrint('blocked load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Blocked Users')),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : _blocked.isEmpty
              ? const Center(child: Text('No blocked users'))
              : ListView.builder(
                  itemCount: _blocked.length,
                  itemBuilder: (context, index) {
                    final pubkey = _blocked[index];
                    return ListTile(
                      leading: const Icon(Icons.block),
                      title: Text(pubkey.substring(0, 24)),
                      trailing: TextButton(
                        onPressed: () async {
                          final session = context.read<SessionService>();
                          final api = context.read<IdentityService>();
                          final me = session.activePubkey;
                          if (me == null) return;
                          try {
                            await api.unblockUser(me, pubkey);
                            setState(() => _blocked.removeAt(index));
                          } catch (e) {
                            if (context.mounted) {
                              ScaffoldMessenger.of(context).showSnackBar(
                                SnackBar(
                                    content:
                                        SelectableText('Unblock error: $e')),
                              );
                            }
                          }
                        },
                        child: const Text('Unblock'),
                      ),
                    );
                  },
                ),
    );
  }
}
