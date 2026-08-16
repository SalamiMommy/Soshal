import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/search_service.dart';
import '../services/session_service.dart';

/// Search: posts, profiles, hashtags and trending (local FTS5).
class SearchScreen extends StatefulWidget {
  /// Search screen.
  const SearchScreen({super.key});

  @override
  State<SearchScreen> createState() => _SearchScreenState();
}

class _SearchScreenState extends State<SearchScreen> {
  final TextEditingController _query = TextEditingController();
  String _mode = 'global';
  bool _loading = false;
  bool _remoteLoading = false;
  List<SearchResultItem> _remoteResults = [];
  Future<List<SearchResultItem>>? _trendingProfilesFuture;
  Future<List<Map<String, dynamic>>>? _trendingHashtagsFuture;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        setState(() {
          _trendingProfilesFuture =
              context.read<SearchService>().trendingProfiles();
          _trendingHashtagsFuture =
              context.read<SearchService>().dbTrendingHashtags();
        });
      }
    });
  }

  @override
  void dispose() {
    _query.dispose();
    super.dispose();
  }

  Future<void> _runSearch() async {
    final q = _query.text.trim();
    if (q.isEmpty) return;
    setState(() => _loading = true);
    try {
      final api = context.read<SearchService>();
      switch (_mode) {
        case 'posts':
          await api.searchPosts(q);
          break;
        case 'profiles':
          await api.searchProfiles(q);
          break;
        case 'hashtags':
          await api.searchHashtags(q);
          break;
        case 'mentions':
          await api.mentions(q);
          break;
        default:
          await api.searchGlobal(q);
      }
      if (mounted && _mode == 'global' && api.results.isEmpty) {
        await _runRemoteSearch();
      }
    } catch (e) {
      debugPrint('search: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  /// Remote NIP-50 search across the account's relays (fallback when local
  /// FTS5 finds nothing, or on explicit button press).
  Future<void> _runRemoteSearch() async {
    final q = _query.text.trim();
    if (q.isEmpty) return;
    setState(() => _remoteLoading = true);
    try {
      final session = context.read<SessionService>();
      final relays = session.activeAccount?.relayList ?? const <String>[];
      if (relays.isEmpty) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(
                content: Text('No relays configured — add relays in Settings')),
          );
        }
        return;
      }
      final json =
          await context.read<SearchService>().remoteGlobalSearch(q, 20, relays);
      final decoded = jsonDecode(json);
      final items = <SearchResultItem>[];
      if (decoded is List) {
        for (final e in decoded) {
          if (e is Map<String, dynamic>) {
            items.add(SearchResultItem(
              id: e['id'] as String? ?? '',
              title: e['content'] as String? ?? '',
              description: e['content'] as String? ?? '',
              pubkey: e['pubkey'] as String?,
              kind: 'post',
              createdAt: (e['created_at'] as num?)?.toInt() ?? 0,
            ));
          }
        }
      }
      if (mounted) setState(() => _remoteResults = items);
    } catch (e) {
      debugPrint('remote search: $e');
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Remote search failed: $e')),
        );
      }
    }
    if (mounted) setState(() => _remoteLoading = false);
  }

  Future<void> _loadTrending() async {
    setState(() => _loading = true);
    try {
      _trendingHashtagsFuture =
          context.read<SearchService>().dbTrendingHashtags();
      await context.read<SearchService>().trendingHashtags();
    } catch (e) {
      debugPrint('trending: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: TextField(
          controller: _query,
          decoration: const InputDecoration(
            hintText: 'Search posts, people, #tags',
            border: InputBorder.none,
          ),
          textInputAction: TextInputAction.search,
          onSubmitted: (_) => _runSearch(),
        ),
        actions: [
          IconButton(
            icon: _remoteLoading
                ? const SizedBox(
                    width: 18,
                    height: 18,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.public),
            tooltip: 'Remote search (relays)',
            onPressed: _remoteLoading ? null : _runRemoteSearch,
          ),
          IconButton(
            icon: const Icon(Icons.search),
            onPressed: _runSearch,
          ),
        ],
      ),
      body: Consumer<SearchService>(
        builder: (context, api, _) {
          if (_loading) {
            return const Center(child: CircularProgressIndicator());
          }
          final q = _query.text.trim();
          final showHashtags = _mode == 'hashtags' || q.isEmpty;
          if (showHashtags && api.hashtags.isNotEmpty && _mode == 'hashtags') {
            return ListView.builder(
              itemCount: api.hashtags.length,
              itemExtent: 56.0,
              itemBuilder: (context, index) {
                final tag = api.hashtags[index];
                return ListTile(
                  leading: const Icon(Icons.tag),
                  title: Text('#$tag'),
                  onTap: () {
                    _query.text = tag;
                    _mode = 'posts';
                    _runSearch();
                  },
                );
              },
            );
          }
          if (api.results.isEmpty && q.isEmpty && _mode == 'global') {
            return ListView(
              children: [
                Padding(
                  padding: const EdgeInsets.all(16),
                  child: Text('Trending hashtags',
                      style: Theme.of(context).textTheme.titleMedium),
                ),
                FutureBuilder<List<Map<String, dynamic>>>(
                  future: _trendingHashtagsFuture,
                  builder: (context, snapshot) {
                    final tags = snapshot.data ?? [];
                    if (tags.isEmpty) {
                      return const Padding(
                        padding: EdgeInsets.all(16),
                        child: Text('Nothing trending yet'),
                      );
                    }
                    return Column(
                      children: [
                        for (final tag in tags)
                          ListTile(
                            leading: const Icon(Icons.trending_up),
                            title: Text('#${tag['tag']}'),
                            onTap: () {
                              final t = tag['tag'] as String? ?? '';
                              if (t.isEmpty) return;
                              _query.text = t;
                              _mode = 'posts';
                              _runSearch();
                            },
                          ),
                      ],
                    );
                  },
                ),
                const SizedBox(height: 8),
                Padding(
                  padding: const EdgeInsets.all(16),
                  child: Text('Trending profiles',
                      style: Theme.of(context).textTheme.titleMedium),
                ),
                FutureBuilder<List<SearchResultItem>>(
                  future: _trendingProfilesFuture,
                  builder: (context, snapshot) {
                    final profiles = snapshot.data ?? [];
                    if (profiles.isEmpty) {
                      return const Padding(
                        padding: EdgeInsets.all(16),
                        child: Text('Nothing yet'),
                      );
                    }
                    return Column(
                      children: [
                        for (final p in profiles)
                          ListTile(
                            leading: const Icon(Icons.person),
                            title: Text(p.title,
                                maxLines: 1, overflow: TextOverflow.ellipsis),
                            subtitle: Text(p.description,
                                maxLines: 2, overflow: TextOverflow.ellipsis),
                            onTap: () {
                              final key = p.pubkey ?? p.id;
                              if (key.isNotEmpty) {
                                context.push('/profile/$key');
                              }
                            },
                          ),
                      ],
                    );
                  },
                ),
              ],
            );
          }
          if (api.results.isEmpty && _remoteResults.isNotEmpty) {
            return ListView.builder(
              itemCount: _remoteResults.length,
              itemBuilder: (context, index) {
                final r = _remoteResults[index];
                return ListTile(
                  leading: const Icon(Icons.article_outlined),
                  title: Text(r.title,
                      maxLines: 2, overflow: TextOverflow.ellipsis),
                  subtitle: Text(
                    r.description,
                    maxLines: 3,
                    overflow: TextOverflow.ellipsis,
                  ),
                  onTap: () {
                    if (r.id.isNotEmpty) {
                      context.push('/post/${r.id}');
                    }
                  },
                );
              },
            );
          }
          if (api.results.isEmpty) {
            return ListView(
              children: const [
                SizedBox(height: 200),
                Center(child: Text('Nothing found')),
              ],
            );
          }
          return ListView.builder(
            itemCount: api.results.length,
            itemBuilder: (context, index) {
              final r = api.results[index];
              final isProfile =
                  r.kind == 'profile' || r.pubkey != null && r.id.isNotEmpty;
              return ListTile(
                leading: Icon(
                  isProfile ? Icons.person : Icons.article_outlined,
                ),
                title:
                    Text(r.title, maxLines: 1, overflow: TextOverflow.ellipsis),
                subtitle: Text(
                  r.description,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                ),
                onTap: () {
                  if (isProfile && (r.pubkey ?? r.id).isNotEmpty) {
                    context.push('/profile/${r.pubkey ?? r.id}');
                  } else {
                    context.push('/post/${r.id}');
                  }
                },
              );
            },
          );
        },
      ),
      bottomNavigationBar: SafeArea(
        child: Row(
          children: [
            for (final mode in const [
              ('global', 'All'),
              ('posts', 'Posts'),
              ('profiles', 'People'),
              ('hashtags', 'Tags'),
              ('mentions', 'Mentions'),
            ])
              Expanded(
                child: InkWell(
                  onTap: () {
                    setState(() => _mode = mode.$1);
                    if (mode.$1 == 'hashtags') {
                      _loadTrending();
                    } else if (_query.text.trim().isNotEmpty) {
                      _runSearch();
                    }
                  },
                  child: Padding(
                    padding: const EdgeInsets.symmetric(vertical: 12),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          mode.$2,
                          style: TextStyle(
                            fontWeight: _mode == mode.$1
                                ? FontWeight.bold
                                : FontWeight.normal,
                            color: _mode == mode.$1
                                ? Theme.of(context).colorScheme.primary
                                : null,
                          ),
                        ),
                        if (_mode == mode.$1)
                          Container(
                            margin: const EdgeInsets.only(top: 4),
                            height: 2,
                            width: 24,
                            color: Theme.of(context).colorScheme.primary,
                          ),
                      ],
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
