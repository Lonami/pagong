mod adaptor;
mod blog;
mod config;
mod feed;
mod post;
mod processor;
mod template;
mod utils;

use adaptor::AdaptorExt;
use post::Post;
use processor::Processor;
use template::HtmlTemplate;

use std::io;

fn main() -> io::Result<()> {
    let mut proc = processor::Processor::new("python", ["demo.py"]).unwrap();
    dbg!(proc.send(processor::ProcessorRule::Css));
    if false {
        return Ok(());
    }
    let config = config::parse_cli_args()?;

    let mut content = config.root.clone();
    content.push(config::SOURCE_PATH);

    let mut dist = config.root.clone();
    dist.push(config::TARGET_PATH);

    let scan = blog::scan_dir(&config, content)?;
    blog::generate_from_scan(&config, scan, dist)?;

    Ok(())
}
