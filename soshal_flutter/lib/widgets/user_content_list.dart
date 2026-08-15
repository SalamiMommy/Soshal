import 'package:flutter/material.dart';
import 'error_state_text.dart';

/// Shared skeleton for user-content list pages (minis, musicloud, …):
/// loading spinner, pull-to-refresh, empty state, separated list and
/// optional error banner. Data-specific rendering stays in the caller's
/// [itemBuilder].
class UserContentList extends StatelessWidget {
  const UserContentList({
    super.key,
    required this.title,
    required this.emptyIcon,
    required this.emptyTitle,
    required this.emptyBody,
    required this.itemCount,
    required this.itemBuilder,
    required this.onRefresh,
    this.loading = false,
    this.error,
  });

  final String title;
  final IconData emptyIcon;
  final String emptyTitle;
  final String emptyBody;
  final int itemCount;
  final Widget Function(BuildContext, int) itemBuilder;
  final Future<void> Function() onRefresh;
  final bool loading;
  final String? error;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(title)),
      body: loading
          ? const Center(child: CircularProgressIndicator())
          : RefreshIndicator(
              onRefresh: onRefresh,
              child: itemCount == 0
                  ? ListView(
                      children: [
                        const SizedBox(height: 120),
                        Center(
                          child: Padding(
                            padding: const EdgeInsets.all(24),
                            child: Column(
                              children: [
                                Icon(
                                  emptyIcon,
                                  size: 56,
                                  color:
                                      Theme.of(context).colorScheme.outline,
                                ),
                                const SizedBox(height: 16),
                                Text(
                                  emptyTitle,
                                  style: Theme.of(context)
                                      .textTheme
                                      .titleLarge,
                                ),
                                const SizedBox(height: 8),
                                Text(
                                  emptyBody,
                                  textAlign: TextAlign.center,
                                  style: TextStyle(
                                      color: Theme.of(context)
                                          .colorScheme
                                          .onSurfaceVariant),
                                ),
                              ],
                            ),
                          ),
                        ),
                      ],
                    )
                  : ListView.separated(
                      itemCount: itemCount,
                      separatorBuilder: (_, __) => const Divider(height: 1),
                      itemBuilder: itemBuilder,
                    ),
            ),
      bottomNavigationBar: error != null && error!.isNotEmpty
          ? Material(
              color: Theme.of(context).colorScheme.errorContainer,
              child: Padding(
                padding: const EdgeInsets.all(12),
                child: ErrorStateText('Error: $error'),
              ),
            )
          : null,
    );
  }
}