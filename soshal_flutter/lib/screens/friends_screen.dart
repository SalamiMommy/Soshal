import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../services/friends_service.dart';
import '../services/messaging_service.dart';
import '../services/session_service.dart';
import '../utils/format.dart';
import '../widgets/error_state_text.dart';

/// Friends: suggestions, add-friend, and in-memory contacts.
class FriendsScreen extends StatefulWidget {
  /// Friends screen.
  const FriendsScreen({super.key});

  @override
  State<FriendsScreen> createState() => _FriendsScreenState();
}

class _FriendsScreenState extends State<FriendsScreen> {
  late final FriendsService _service;
  final TextEditingController _addQuery = TextEditingController();
  final TextEditingController _contactFilter = TextEditingController();
  List<ProfileInfo> _searchResults = [];
  bool _searching = false;
  final Set<String> _sending = {};
  bool _refreshingFollows = false;
  String? _followsInfo;

  @override
  void initState() {
    super.initState();
    _service = context.read<FriendsService>();
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
    _addQuery.dispose();
    _contactFilter.dispose();
    super.dispose();
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

  Future<void> _refreshFromRelays() async {
    final myPubkey = context.read<SessionService>().activePubkey;
    if (myPubkey == null) return;
    setState(() => _refreshingFollows = true);
    try {
      final json = await _service.fetchFollows(myPubkey);
      final follows = jsonDecode(json) as List<dynamic>;
      if (mounted) {
        setState(() => _followsInfo =
            '${follows.length} follow(s) from relays · ${json.length} B');
      }
    } catch (e) {
      debugPrint('refresh follows: $e');
      if (mounted) {
        setState(() => _followsInfo = 'Refresh failed: $e');
      }
    }
    if (mounted) setState(() => _refreshingFollows = false);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      appBar: AppBar(title: const Text('Friends')),
      body: ListenableBuilder(
        listenable: _service,
        builder: (context, _) {
          return CustomScrollView(
            slivers: [
              SliverToBoxAdapter(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
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
                          'No suggestions yet — friend discovery arrives with '
                          'the backend.',
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
                                    child:
                                        CircularProgressIndicator(strokeWidth: 2),
                                  )
                                : const Icon(Icons.search),
                            onPressed: _searching ? null : _runSearch,
                          ),
                        ],
                      ),
                    ),
                    const Divider(height: 32),
                    _sectionTitle(
                        theme, 'My contacts (${_service.contacts.length})'),
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
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 16),
                      child: Row(
                        children: [
                          TextButton.icon(
                            onPressed:
                                _refreshingFollows ? null : _refreshFromRelays,
                            icon: _refreshingFollows
                                ? const SizedBox(
                                    width: 18,
                                    height: 18,
                                    child: CircularProgressIndicator(
                                        strokeWidth: 2),
                                  )
                                : const Icon(Icons.sync),
                            label: const Text('Refresh from relays'),
                          ),
                        ],
                      ),
                    ),
                    if (_followsInfo != null)
                      ListTile(
                        dense: true,
                        title: Text(_followsInfo!),
                        trailing: IconButton(
                          icon: const Icon(Icons.close),
                          tooltip: 'Clear',
                          onPressed: () => setState(() => _followsInfo = null),
                        ),
                      ),
                  ],
                ),
              ),
              if (_service.suggestions.isNotEmpty)
                SliverList.separated(
                  itemCount: _service.suggestions.length,
                  itemBuilder: (context, i) {
                    final pk = _service.suggestions[i];
                    return ListTile(
                      leading: const Icon(Icons.person_outline),
                      title: Text(shortPubkey(pk)),
                      trailing: _sending.contains(pk)
                          ? const SizedBox(
                              width: 20,
                              height: 20,
                              child:
                                  CircularProgressIndicator(strokeWidth: 2),
                            )
                          : TextButton(
                              onPressed: () => _sendRequest(pk),
                              child: const Text('Send request'),
                            ),
                    );
                  },
                  separatorBuilder: (context, i) => const Divider(height: 1),
                ),
              if (_searchResults.isNotEmpty)
                SliverList.separated(
                  itemCount: _searchResults.length,
                  itemBuilder: (context, i) {
                    final p = _searchResults[i];
                    return ListTile(
                      leading: const Icon(Icons.person),
                      title: Text(
                          p.displayName.isNotEmpty ? p.displayName : p.name,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis),
                      subtitle: Text(shortPubkey(p.pubkey)),
                      trailing: p.isFollowing ||
                              _service.contacts
                                  .any((c) => c.pubkey == p.pubkey)
                          ? const Icon(Icons.check, size: 18)
                          : TextButton(
                              onPressed: () => _follow(p),
                              child: const Text('Follow'),
                            ),
                    );
                  },
                  separatorBuilder: (context, i) => const Divider(height: 1),
                ),
              if (_service.contacts.isEmpty)
                const SliverToBoxAdapter(
                  child: Padding(
                    padding: EdgeInsets.all(16),
                    child: Text(
                      'No contacts yet — follow someone from Add friend above. '
                      'Contacts live in memory only for now.',
                    ),
                  ),
                )
              else
                SliverList.separated(
                  itemCount: _service.contacts.length,
                  itemBuilder: (context, i) {
                    final c = _service.contacts[i];
                    final filter = _contactFilter.text.trim();
                    if (filter.isNotEmpty &&
                        !c.pubkey.contains(filter) &&
                        !c.name.contains(filter) &&
                        !c.displayName.contains(filter)) {
                      return const SizedBox.shrink();
                    }
                    return ListTile(
                      leading: const Icon(Icons.person_outline),
                      title: Text(
                        c.displayName.isNotEmpty ? c.displayName : c.name,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                      subtitle: Text(shortPubkey(c.pubkey)),
                      trailing: IconButton(
                        icon: const Icon(Icons.remove_circle_outline),
                        tooltip: 'Remove contact',
                        onPressed: () => _service.removeContact(c.pubkey),
                      ),
                    );
                  },
                  separatorBuilder: (context, i) => const Divider(height: 1),
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
