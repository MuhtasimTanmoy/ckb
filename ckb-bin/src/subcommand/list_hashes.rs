use ckb_app_config::{cli, CKBAppConfig, ExitCode};
use ckb_chain_spec::ChainSpec;
use ckb_resource::{Resource, AVAILABLE_SPECS};
use ckb_types::{packed::CellOutput, prelude::*, H256};
use ckb_util::LinkedHashMap;
use clap::ArgMatches;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SystemCell {
    pub path: String,
    pub tx_hash: H256,
    pub index: usize,
    pub data_hash: H256,
    pub type_hash: Option<H256>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DepGroupCell {
    pub included_cells: Vec<String>,
    pub tx_hash: H256,
    pub index: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SpecHashes {
    pub spec_hash: H256,
    pub genesis: H256,
    pub cellbase: H256,
    pub system_cells: Vec<SystemCell>,
    pub dep_groups: Vec<DepGroupCell>,
}

impl TryFrom<ChainSpec> for SpecHashes {
    type Error = ExitCode;

    fn try_from(mut spec: ChainSpec) -> Result<Self, Self::Error> {
        let hash_option = spec.genesis.hash.take();
        let consensus = spec.build_consensus().map_err(to_config_error)?;
        if let Some(hash) = hash_option {
            let genesis_hash: H256 = consensus.genesis_hash().unpack();
            if hash != genesis_hash {
                eprintln!(
                    "Genesis hash unmatched in {} chainspec config file:\n\
                     in file {:#x},\n\
                     actual {:#x}",
                    spec.name, hash, genesis_hash
                );
            }
        }

        let block = consensus.genesis_block();
        let cellbase = &block.transactions()[0];
        let dep_group_tx = &block.transactions()[1];

        // Zip name with the transaction outputs. System cells start from 1 in the genesis cellbase outputs.
        let cells_hashes = spec
            .genesis
            .system_cells
            .iter()
            .map(|system_cell| &system_cell.file)
            .zip(
                cellbase
                    .outputs()
                    .into_iter()
                    .zip(cellbase.outputs_data().into_iter())
                    .skip(1),
            )
            .enumerate()
            .map(|(index_minus_one, (resource, (output, data)))| {
                let data_hash: H256 = CellOutput::calc_data_hash(&data.raw_data()).unpack();
                let type_hash: Option<H256> = output
                    .type_()
                    .to_opt()
                    .map(|script| script.calc_script_hash().unpack());
                SystemCell {
                    path: resource.to_string(),
                    tx_hash: cellbase.hash().unpack(),
                    index: index_minus_one + 1,
                    data_hash,
                    type_hash,
                }
            })
            .collect();

        let dep_groups = spec
            .genesis
            .dep_groups
            .iter()
            .enumerate()
            .map(|(index, dep_group)| DepGroupCell {
                included_cells: dep_group
                    .files
                    .iter()
                    .map(|res| res.to_string())
                    .collect::<Vec<_>>(),
                tx_hash: dep_group_tx.hash().unpack(),
                index,
            })
            .collect::<Vec<_>>();

        Ok(SpecHashes {
            spec_hash: spec.hash.unpack(),
            genesis: consensus.genesis_hash().unpack(),
            cellbase: cellbase.hash().unpack(),
            system_cells: cells_hashes,
            dep_groups,
        })
    }
}

pub fn list_hashes(root_dir: PathBuf, matches: &ArgMatches) -> Result<(), ExitCode> {
    let mut specs = Vec::new();

    let output_format = matches.get_one::<String>(cli::ARG_FORMAT).unwrap().as_str();

    if matches.contains_id(cli::ARG_BUNDLED) {
        if output_format == "toml" {
            println!("# Generated by: ckb list-hashes -b\n");
        }

        for env in AVAILABLE_SPECS {
            let spec = ChainSpec::load_from(&Resource::bundled(format!("specs/{env}.toml")))
                .map_err(to_config_error)?;
            let spec_name = spec.name.clone();
            let spec_hashes: SpecHashes = spec.try_into()?;
            specs.push((spec_name, spec_hashes));
        }
    } else {
        if output_format == "toml" {
            println!("# Generated by: ckb list-hashes");
        }
        let mut resource = Resource::ckb_config(&root_dir);
        if !resource.exists() {
            resource = Resource::bundled_ckb_config();
        }

        let mut config = CKBAppConfig::load_from_slice(&resource.get()?)?;
        config.chain.spec.absolutize(&root_dir);
        let chain_spec = ChainSpec::load_from(&config.chain.spec).map_err(to_config_error)?;
        let spec_name = chain_spec.name.clone();
        let spec_hashes: SpecHashes = chain_spec.try_into()?;
        specs.push((spec_name, spec_hashes));
    }

    let mut map = LinkedHashMap::new();
    for (spec_name, spec_hashes) in specs {
        map.insert(spec_name, spec_hashes);
    }

    match output_format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&map).unwrap());
        }
        _ => {
            println!("{}", toml::to_string(&map).unwrap());
        }
    }

    Ok(())
}

fn to_config_error(err: Box<dyn std::error::Error>) -> ExitCode {
    eprintln!("ERROR: {err}");
    ExitCode::Config
}
