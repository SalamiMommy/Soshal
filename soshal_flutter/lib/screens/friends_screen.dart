import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../services/friends_service.dart';
import '../services/messaging_service.dart';
import '../services/session_service.dart';
import '../widgets/error_state_text.dart';

/// Friends: suggestions, add-friend, and in-memory contacts.
class FriendsScreen extends StatefulWidget {
  /// Friends screen.
  const FriendsScreen({super.key});

  @override
  State<FriendsScreen> createState() => _FriendsScreenState();
}

class _FriendsScreenState extends State<FriendsScreen> {
  final FriendsService _service = FriendsService();
  final TextEditingController _addQuery = TextEditingController();
  final TextEditingController _contactFilter = TextEditingController();
  List<ProfileInfo> _searchResults = [];
  bool _searching = false;
  final Set<String> _sending = {};

  @override
  void initState() {
    super.initState();
    _loadSuggestions();
  }

  Future<void> _loadSuggestions() async {
    try {
      await _service.fetchSuggestions();
    } catch (e) {
      debugPrint('suggestions: $e');
    }
  }

  @override
  void dispose() {
    _service.dispose();
    _addQuery.dispose();
    _contactFilter.dispose();
    super.dispose();
  }

  static String _short(String pubkey) {
    if (pubkey.length <= 12) return pubkey;
    return '${pubkey.substring(0, 6)}…${pubkey.substring(pubkey.length - 6)}';
  }

  Future<void> _runSearch() async {
    final q = _addQuery.text.trim();
    if (q.isEmpty) return;
    setState(() => _searching = true);
    try {
      final results = await context.read<IdentityService>().searchUsers(q);
      if (mounted) setState(() => _searchResults = results);
    } catch (e) {
      debugPrint('add friend search: $e');
    }
    if (mounted) setState(() => _searching = false);
  }

  Future<void> _sendRequest(String pubkey) async {
    setState(() => _sending.add(pubkey));
    try {
      final ok = await _service.sendFriendRequest(pubkey);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(ok
                ? 'Friend request sent'
                : 'Friend request could not be sent'),
          ),
        );
      }
    } catch (e) {
      debugPrint('send friend request: $e');
    }
    if (mounted) setState(() => _sending.remove(pubkey));
  }

  Future<void> _follow(ProfileInfo profile) async {
    final myPubkey = context.read<SessionService>().activePubkey;
    if (myPubkey == null) return;
    try {
      await context
          .read<IdentityService>()
          .followUser(profile.pubkey, myPubkey);
      _service.addContact(profile);
    } catch (e) {
      debugPrint('follow: $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      appBar: AppBar(title: const Text('Friends')),
      body: ListenableBuilder(
        listenable: _service,
        builder: (context, _) {
          return ListView(
            padding: const EdgeInsets.symmetric(vertical: 8),
            children: [
              _sectionTitle(theme, 'People you may know'),
              if (_service.lastError != null)
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: ErrorStateText(_service.lastError!),
                )
              else if (_service.suggestions.isEmpty)
                const Padding(
                  padding: EdgeInsets.all(16),
                  child: Text(
                    'No suggestions yet — friend discovery arrives with the backend.',
                  ),
                )
              else
                for (final pk in _service.suggestions)
                  ListTile(
                    leading: const Icon(Icons.person_outline),
                    title: Text(_short(pk)),
                    trailing: _sending.contains(pk)
                        ? const SizedBox(
                            width: 20,
                            height: 20,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : TextButton(
                            onPressed: () => _sendRequest(pk),
                            child: const Text('Send request'),
                          ),
                  ),
              const Divider(height: 32),
              _sectionTitle(theme, 'Add friend'),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: Row(
                  children: [
                    Expanded(
                      child: TextField(
                        controller: _addQuery,
                        decoration: const InputDecoration(
                          hintText: 'npub or hex pubkey…',
                          border: OutlineInputBorder(),
                          isDense: true,
                        ),
                        textInputAction: TextInputAction.search,
                        onSubmitted: (_) => _runSearch(),
                      ),
                    ),
                    const SizedBox(width: 8),
                    IconButton(
                      icon: _searching
                          ? const SizedBox(
                              width: 20,
                              height: 20,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(Icons.search),
                      onPressed: _searching ? null : _runSearch,
                    ),
                  ],
                ),
              ),
              if (_searchResults.isNotEmpty)
                for (final p in _searchResults)
                  ListTile(
                    leading: const Icon(Icons.person),
                    title: Text(
                        p.displayName.isNotEmpty ? p.displayName : p.name,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis),
                    subtitle: Text(_short(p.pubkey)),
                    trailing: p.isFollowing ||
                            _service.contacts.any((c) => c.pubkey == p.pubkey)
                        ? const Icon(Icons.check, size: 18)
                        : TextButton(
                            onPressed: () => _follow(p),
                            child: const Text('Follow'),
                          ),
                  ),
              const Divider(height: 32),
              _sectionTitle(theme, 'My contacts (${_service.contacts.length})'),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: TextField(
                  controller: _contactFilter,
                  decoration: const InputDecoration(
                    hintText: 'Search contacts…',
                    border: OutlineInputBorder(),
                    isDense: true,
                  ),
                ),
              ),
              if (_service.contacts.isEmpty)
                const Padding(
                  padding: EdgeInsets.all(16),
                  child: Text(
                    'No contacts yet — follow someone from Add friend above. '
                    'Contacts live in memory only for now.',
                  ),
                )
              else
                for (final c in _service.contacts)
                  if (c.pubkey.contains(_contactFilter.text.trim()) ||
                      c.name.contains(_contactFilter.text.trim()) ||
                      c.displayName.contains(_contactFilter.text.trim()))
                    ListTile(
                      leading: const Icon(Icons.person_outline),
                      title: Text(
                        c.displayName.isNotEmpty ? c.displayName : c.name,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                      subtitle: Text(_short(c.pubkey)),
                      trailing: IconButton(
                        icon: const Icon(Icons.remove_circle_outline),
                        tooltip: 'Remove contact',
                        onPressed: () => _service.removeContact(c.pubkey),
                      ),
                    ),
            ],
          );
        },
      ),
    );
  }

  Widget _sectionTitle(ThemeData theme, String title) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 8, 16, 8),
      child: Text(title, style: theme.textTheme.titleMedium),
    );
  }
}
