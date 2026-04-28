import SwiftUI

/// Sidebar list of conversations: select to switch, "+" to add, context
/// menu for rename/delete. Bound to `ConversationTabsViewModel`.
struct ConversationSidebarView: View {
    @Bindable var viewModel: ConversationTabsViewModel
    @State private var showingNewConversationSheet = false
    @State private var renamingConversation: Conversation? = nil
    @State private var deletingConversation: Conversation? = nil
    let providers: [Provider]
    let models: [String: [Model]]

    var body: some View {
        VStack(spacing: 0) {
            List(selection: selectionBinding) {
                Section {
                    ForEach(viewModel.conversations, id: \.id) { conversation in
                        ConversationRow(conversation: conversation)
                            .tag(conversation.id)
                            .contextMenu {
                                Button("Rename") { renamingConversation = conversation }
                                Button("Delete", role: .destructive) {
                                    deletingConversation = conversation
                                }
                            }
                    }
                } header: {
                    Text("Conversations")
                }
            }
            .listStyle(.sidebar)

            Divider()

            HStack {
                Button {
                    showingNewConversationSheet = true
                } label: {
                    Label("New Conversation", systemImage: "plus")
                }
                .buttonStyle(.plain)
                .keyboardShortcut("n", modifiers: .command)
                Spacer()
            }
            .padding(8)
        }
        .sheet(isPresented: $showingNewConversationSheet) {
            NewConversationModal(
                providers: providers,
                models: models,
                sandboxTemplates: viewModel.sandboxTemplates
            ) { providerId, model, title, sandboxTemplateId in
                showingNewConversationSheet = false
                Task {
                    await viewModel.createConversation(
                        providerId: providerId,
                        model: model,
                        title: title,
                        sandboxTemplateId: sandboxTemplateId
                    )
                }
            } onCancel: {
                showingNewConversationSheet = false
            }
        }
        .sheet(item: $renamingConversation) { conversation in
            RenameConversationSheet(
                conversation: conversation,
                onRename: { newTitle in
                    let id = conversation.id
                    renamingConversation = nil
                    Task { await viewModel.rename(id, to: newTitle) }
                },
                onCancel: { renamingConversation = nil }
            )
        }
        .alert(
            "Delete conversation?",
            isPresented: deletingPresentation,
            presenting: deletingConversation
        ) { conversation in
            Button("Delete", role: .destructive) {
                let id = conversation.id
                deletingConversation = nil
                Task { await viewModel.delete(id) }
            }
            Button("Cancel", role: .cancel) {
                deletingConversation = nil
            }
        } message: { conversation in
            Text("\(conversation.title) and all its messages will be permanently removed.")
        }
    }

    private var selectionBinding: Binding<String?> {
        Binding(
            get: { viewModel.activeConversationId },
            set: { newValue in
                if let id = newValue { viewModel.select(id) }
            }
        )
    }

    private var deletingPresentation: Binding<Bool> {
        Binding(
            get: { deletingConversation != nil },
            set: { if !$0 { deletingConversation = nil } }
        )
    }
}

extension Conversation: Identifiable {}

private struct ConversationRow: View {
    let conversation: Conversation

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: conversation.sandboxTemplateId != nil
                  ? "lock.shield.fill"
                  : "bubble.left.and.bubble.right.fill")
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 2) {
                Text(conversation.title)
                    .font(.body)
                    .lineLimit(1)
                Text(conversation.model)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .padding(.vertical, 2)
    }
}
