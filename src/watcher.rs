use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use tokio::runtime::Runtime;
use log::{error, info};
use watchexec::Watchexec;
use watchexec_events::{Event, Tag, Source};

use crate::error::StorageError;
use crate::state::PipelineEvent;

/// Process a watchexec event and send file change notifications
fn process_event(event: &Event, tx: &mpsc::Sender<PipelineEvent>, _base_path: &PathBuf) {
    // Filter for filesystem events only
    let is_filesystem = event.tags.iter().any(|tag| {
        matches!(tag, Tag::Source(Source::Filesystem))
    });
    
    if !is_filesystem {
        return;
    }
    
    // Extract all changed paths from the event
    for tag in &event.tags {
        if let Tag::Path { path, file_type } = tag {
            // Log detailed change information
            let type_str = file_type.as_ref()
                .map(|ft| format!("{:?}", ft))
                .unwrap_or_else(|| "unknown".to_string());
            
            info!("File changed: {} (type: {})", path.display(), type_str);
            
            // Send specific file path that changed
            if let Err(e) = tx.send(PipelineEvent::FileChanged(path.clone())) {
                error!("Failed to send file change event: {}", e);
            }
        }
    }
}

// Event-based file watcher using watchexec
pub fn watch_directory(path: PathBuf, tx: mpsc::Sender<PipelineEvent>) -> Result<(), StorageError> {
    info!("Starting file watcher for directory: {}", path.display());

    // Clone the path and sender for the handler
    let path_to_watch = path.clone();
    let tx_clone = tx.clone();

    // Start the watcher in a background thread
    thread::spawn(move || {
        info!(
            "File watcher thread started for {}",
            path_to_watch.display()
        );

        // Create tokio runtime for watchexec
        match Runtime::new() {
            Ok(rt) => {
                rt.block_on(async {
                    // Initialize watchexec with event handler
                    match Watchexec::new(move |mut action| {
                        // Process filesystem events
                        for event in action.events.iter() {
                            process_event(event, &tx_clone, &path_to_watch);
                        }
                        
                        // Handle shutdown signals
                        if action.signals().next().is_some() {
                            info!("Received shutdown signal, stopping file watcher");
                            action.quit();
                        }
                        
                        action
                    }) {
                        Ok(wx) => {
                            // Configure path to watch
                            wx.config.pathset([path_to_watch.clone()]);
                            
                            // Start watchexec main loop
                            info!("Event-based file watcher started for {}", path_to_watch.display());
                            match wx.main().await {
                                Ok(_) => info!("File watcher completed successfully"),
                                Err(e) => error!("File watcher error: {}", e),
                            }
                        }
                        Err(e) => error!("Failed to initialize watchexec: {}", e),
                    }
                });
            }
            Err(e) => error!("Failed to create runtime: {}", e),
        }

        info!("File watcher thread exited");
    });

    info!("File watcher initialized successfully");
    Ok(())
}
